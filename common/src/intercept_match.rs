//
// Shared intercept-rule matching. Service ingest, CLI form preview, and
// any other client must agree on which fields a pattern hits.
//

use regex::Regex;

use crate::messaging::{
    traffic_agent_matches, InterceptRule, InterceptedTrafficEntry, RuleScope, TargetDirection,
    TrafficDirection,
};

/// Match using a precompiled regex + rule direction/scope against one entry.
pub fn rule_matches_entry(
    rule: &InterceptRule,
    regex: &Regex,
    entry: &InterceptedTrafficEntry,
) -> bool {
    if !direction_allows(&rule.target_direction, &entry.direction) {
        return false;
    }
    if !scope_allows(&rule.scope, entry) {
        return false;
    }
    pattern_matches_entry(regex, entry, &rule.target_direction)
}

/// Pattern-only match (no scope). Used by CLI form preview where scope
/// pickers may be incomplete while typing.
pub fn pattern_matches_entry(
    regex: &Regex,
    entry: &InterceptedTrafficEntry,
    direction: &TargetDirection,
) -> bool {
    if !direction_allows(direction, &entry.direction) {
        return false;
    }

    if regex.is_match(&entry.url) {
        return true;
    }
    if regex.is_match(&entry.host) {
        return true;
    }
    if let Some(method) = entry.method.as_deref()
        && regex.is_match(method)
    {
        return true;
    }

    if *direction != TargetDirection::Receive {
        if headers_match(regex, entry.request_headers.as_ref()) {
            return true;
        }
        if body_match(regex, entry.request_body.as_deref()) {
            return true;
        }
    }

    if *direction != TargetDirection::Send {
        if let Some(status) = entry.response_status
            && regex.is_match(&status.to_string())
        {
            return true;
        }
        if headers_match(regex, entry.response_headers.as_ref()) {
            return true;
        }
        if body_match(regex, entry.response_body.as_deref()) {
            return true;
        }
    }

    false
}

fn direction_allows(target: &TargetDirection, actual: &TrafficDirection) -> bool {
    match target {
        TargetDirection::Send => *actual == TrafficDirection::Send,
        TargetDirection::Receive => *actual == TrafficDirection::Receive,
        TargetDirection::Both => true,
    }
}

fn scope_allows(scope: &RuleScope, entry: &InterceptedTrafficEntry) -> bool {
    match scope {
        RuleScope::All => true,
        RuleScope::Node { node_id } => entry.node_id == *node_id,
        RuleScope::Agent {
            node_id,
            agent_short_name,
        } => {
            entry.node_id == *node_id
                && traffic_agent_matches(&entry.agent_short_name, agent_short_name)
        }
    }
}

fn headers_match(regex: &Regex, headers: Option<&indexmap::IndexMap<String, String>>) -> bool {
    let Some(headers) = headers else {
        return false;
    };
    for (key, value) in headers {
        if regex.is_match(key) || regex.is_match(value) {
            return true;
        }
    }
    false
}

fn body_match(regex: &Regex, body: Option<&[u8]>) -> bool {
    let Some(body) = body else {
        return false;
    };
    let Ok(body_str) = std::str::from_utf8(body) else {
        return false;
    };
    regex.is_match(body_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::InterceptMethod;
    use chrono::Utc;
    use indexmap::IndexMap;

    fn base_entry() -> InterceptedTrafficEntry {
        InterceptedTrafficEntry {
            id: Some(1),
            timestamp: Utc::now(),
            node_id: "n1".into(),
            agent_short_name: "claude".into(),
            intercept_method: InterceptMethod::Proxy,
            direction: TrafficDirection::Send,
            method: Some("POST".into()),
            url: "https://api.anthropic.com/v1/messages".into(),
            host: "api.anthropic.com".into(),
            request_headers: None,
            request_body: None,
            response_status: None,
            response_headers: None,
            response_body: None,
        }
    }

    fn entry_with_body(url: &str, body: &str) -> InterceptedTrafficEntry {
        let mut e = base_entry();
        e.url = url.into();
        e.request_body = Some(body.as_bytes().to_vec());
        e
    }

    fn sample_rule(pattern: &str, direction: TargetDirection, scope: RuleScope) -> InterceptRule {
        let now = Utc::now();
        InterceptRule {
            id: 1,
            name: "t".into(),
            regex_pattern: pattern.into(),
            target_direction: direction,
            scope,
            enabled: true,
            summarization_prompt: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn case_insensitive_body_match() {
        let re = Regex::new(r"(?i)system").unwrap();
        let entry = entry_with_body(
            "https://api.anthropic.com/v1/messages",
            r#"{"text":"<system-reminder>hello"}"#,
        );
        assert!(pattern_matches_entry(&re, &entry, &TargetDirection::Both));
        assert!(!pattern_matches_entry(
            &re,
            &entry_with_body("https://api.anthropic.com/v1/messages", r#"{"ok":true}"#),
            &TargetDirection::Both
        ));
    }

    #[test]
    fn case_insensitive_flag_matches_mixed_case_body() {
        let re = Regex::new(r"(?i)system").unwrap();
        let entry = entry_with_body(
            "https://api.anthropic.com/v1/messages",
            r#"{"role":"SYSTEM","content":"x"}"#,
        );
        assert!(pattern_matches_entry(&re, &entry, &TargetDirection::Both));
        // Without the flag, ASCII "SYSTEM" must not match lowercase pattern.
        let re_cs = Regex::new(r"system").unwrap();
        assert!(!pattern_matches_entry(
            &re_cs,
            &entry,
            &TargetDirection::Both
        ));
    }

    #[test]
    fn send_direction_skips_response_only_fields() {
        let re = Regex::new(r"secret-token").unwrap();
        let mut entry = entry_with_body("https://example.com/", "{}");
        entry.request_body = None;
        entry.response_body = Some(b"secret-token".to_vec());
        // Send traffic: response body is still searched for Both, not for Send.
        assert!(!pattern_matches_entry(
            &re,
            &entry,
            &TargetDirection::Send
        ));
        assert!(pattern_matches_entry(
            &re,
            &entry,
            &TargetDirection::Both
        ));
        // Receive-only rule rejects Send-direction entries entirely.
        assert!(!pattern_matches_entry(
            &re,
            &entry,
            &TargetDirection::Receive
        ));
        entry.direction = TrafficDirection::Receive;
        assert!(pattern_matches_entry(
            &re,
            &entry,
            &TargetDirection::Receive
        ));
    }

    #[test]
    fn receive_direction_skips_request_only_fields() {
        let re = Regex::new(r"prompt-secret").unwrap();
        let mut entry = entry_with_body("https://example.com/", "prompt-secret");
        entry.direction = TrafficDirection::Receive;
        entry.response_body = Some(b"ok".to_vec());
        // Entry is Receive traffic; Send-only rule rejects by direction.
        assert!(!pattern_matches_entry(
            &re,
            &entry,
            &TargetDirection::Send
        ));
        // Receive rule: request body is not searched.
        assert!(!pattern_matches_entry(
            &re,
            &entry,
            &TargetDirection::Receive
        ));
        entry.response_body = Some(b"prompt-secret in response".to_vec());
        assert!(pattern_matches_entry(
            &re,
            &entry,
            &TargetDirection::Receive
        ));
    }

    #[test]
    fn url_match_still_works() {
        let re = Regex::new(r"feature-flags").unwrap();
        let entry = entry_with_body("https://api.factory.ai/api/feature-flags", "{}");
        assert!(pattern_matches_entry(
            &re,
            &entry,
            &TargetDirection::Both
        ));
    }

    #[test]
    fn host_and_method_match() {
        let re_host = Regex::new(r"anthropic").unwrap();
        let re_method = Regex::new(r"^POST$").unwrap();
        let entry = base_entry();
        assert!(pattern_matches_entry(
            &re_host,
            &entry,
            &TargetDirection::Both
        ));
        assert!(pattern_matches_entry(
            &re_method,
            &entry,
            &TargetDirection::Both
        ));
        assert!(!pattern_matches_entry(
            &Regex::new(r"^GET$").unwrap(),
            &entry,
            &TargetDirection::Both
        ));
    }

    #[test]
    fn request_and_response_header_match() {
        let re = Regex::new(r"(?i)x-api-key").unwrap();
        let mut entry = base_entry();
        let mut headers = IndexMap::new();
        headers.insert("X-Api-Key".into(), "sk-test".into());
        entry.request_headers = Some(headers);
        assert!(pattern_matches_entry(&re, &entry, &TargetDirection::Both));
        assert!(pattern_matches_entry(&re, &entry, &TargetDirection::Send));
        assert!(!pattern_matches_entry(
            &re,
            &entry,
            &TargetDirection::Receive
        ));

        let re_val = Regex::new(r"sk-test").unwrap();
        assert!(pattern_matches_entry(
            &re_val,
            &entry,
            &TargetDirection::Both
        ));

        let mut entry2 = base_entry();
        entry2.direction = TrafficDirection::Receive;
        let mut resp = IndexMap::new();
        resp.insert("content-type".into(), "application/json".into());
        entry2.response_headers = Some(resp);
        assert!(pattern_matches_entry(
            &Regex::new(r"application/json").unwrap(),
            &entry2,
            &TargetDirection::Receive
        ));
    }

    #[test]
    fn response_status_match() {
        let re = Regex::new(r"^429$").unwrap();
        let mut entry = base_entry();
        entry.direction = TrafficDirection::Receive;
        entry.response_status = Some(429);
        assert!(pattern_matches_entry(
            &re,
            &entry,
            &TargetDirection::Receive
        ));
        assert!(!pattern_matches_entry(&re, &entry, &TargetDirection::Send));
    }

    #[test]
    fn non_utf8_body_is_not_matched_as_text() {
        // Pattern only present in the binary body bytes (as latin1-ish
        // content if forced); not in URL/host/method. Invalid UTF-8 body
        // must not be decoded as text and matched.
        let re = Regex::new(r"only-in-body-xyz").unwrap();
        let mut entry = base_entry();
        entry.url = "https://example.com/nope".into();
        entry.host = "example.com".into();
        entry.method = Some("GET".into());
        let mut body = b"prefix ".to_vec();
        body.extend_from_slice(&[0xff, 0xfe, 0xfd]);
        body.extend_from_slice(b" only-in-body-xyz");
        entry.request_body = Some(body);
        assert!(
            !pattern_matches_entry(&re, &entry, &TargetDirection::Both),
            "invalid UTF-8 body must not match via lossy text decode"
        );
    }

    #[test]
    fn rule_scope_all_node_agent() {
        let re = Regex::new(r"messages").unwrap();
        let entry = base_entry();

        let all = sample_rule("messages", TargetDirection::Both, RuleScope::All);
        assert!(rule_matches_entry(&all, &re, &entry));

        let node_ok = sample_rule(
            "messages",
            TargetDirection::Both,
            RuleScope::Node {
                node_id: "n1".into(),
            },
        );
        assert!(rule_matches_entry(&node_ok, &re, &entry));

        let node_miss = sample_rule(
            "messages",
            TargetDirection::Both,
            RuleScope::Node {
                node_id: "other".into(),
            },
        );
        assert!(!rule_matches_entry(&node_miss, &re, &entry));

        let agent_ok = sample_rule(
            "messages",
            TargetDirection::Both,
            RuleScope::Agent {
                node_id: "n1".into(),
                agent_short_name: "claude".into(),
            },
        );
        assert!(rule_matches_entry(&agent_ok, &re, &entry));

        let agent_miss = sample_rule(
            "messages",
            TargetDirection::Both,
            RuleScope::Agent {
                node_id: "n1".into(),
                agent_short_name: "cursor".into(),
            },
        );
        assert!(!rule_matches_entry(&agent_miss, &re, &entry));
    }

    #[test]
    fn rule_direction_filters_entry_direction() {
        let re = Regex::new(r"messages").unwrap();
        let mut send = base_entry();
        send.direction = TrafficDirection::Send;
        let mut recv = base_entry();
        recv.direction = TrafficDirection::Receive;
        recv.response_status = Some(200);

        let send_only = sample_rule("messages", TargetDirection::Send, RuleScope::All);
        assert!(rule_matches_entry(&send_only, &re, &send));
        assert!(!rule_matches_entry(&send_only, &re, &recv));

        let recv_only = sample_rule("messages", TargetDirection::Receive, RuleScope::All);
        assert!(!rule_matches_entry(&recv_only, &re, &send));
        assert!(rule_matches_entry(&recv_only, &re, &recv));
    }

    #[test]
    fn anthropic_system_reminder_scenario() {
        //
        // Regression: user rule `(?i)system` against Anthropic messages
        // body containing `<system-reminder>` must match; URL alone must not.
        //
        let re = Regex::new(r"(?i)system").unwrap();
        let mut entry = base_entry();
        entry.url = "https://api.anthropic.com/v1/messages?beta=true".into();
        entry.method = Some("POST".into());
        entry.request_body = Some(
            br#"{"messages":[{"content":[{"text":"<system-reminder>\nAs you answer"}]}]}"#
                .to_vec(),
        );
        let rule = sample_rule(r"(?i)system", TargetDirection::Both, RuleScope::All);
        assert!(
            rule_matches_entry(&rule, &re, &entry),
            "body with system-reminder must match (?i)system"
        );
        entry.request_body = None;
        assert!(
            !rule_matches_entry(&rule, &re, &entry),
            "URL without system must not match"
        );
    }
}
