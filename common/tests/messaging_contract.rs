use chrono::Utc;
use common::{
    AgentCommand, ClientBroadcastMessage, ClientDirectMessage, ClientSignalMessage, CommandRequest,
    InterceptMethod, InterceptRule, InterceptStatus, InterceptedTrafficEntry, NodeCommand,
    NodeDirectMessage, NodeRegistrationAck, NodeSignalMessage, RuleScope, SemanticParserRequest,
    SystemState, TargetDirection, TrafficDirection,
};
use indexmap::IndexMap;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::json;

fn assert_roundtrip<T>(value: &T)
where
    T: Serialize + DeserializeOwned,
{
    let serialized = serde_json::to_value(value).expect("serialize");
    let decoded: T = serde_json::from_value(serialized.clone()).expect("deserialize");
    let reserialized = serde_json::to_value(decoded).expect("reserialize");
    assert_eq!(serialized, reserialized);
}

#[test]
fn client_signal_command_roundtrip() {
    let msg = ClientSignalMessage::Command(CommandRequest {
        command_id: "cmd-1".to_string(),
        client_id: "client-1".to_string(),
        node_id: "node-1".to_string(),
        command: NodeCommand::Agent(AgentCommand::Select {
            short_name: "claudecode".to_string(),
        }),
    });

    assert_roundtrip(&msg);
}

#[test]
fn node_signal_semantic_parser_roundtrip() {
    let msg = NodeSignalMessage::SemanticParserRequest {
        node_id: "node-1".to_string(),
        request: SemanticParserRequest {
            request_id: "req-1".to_string(),
            instruction: "extract hosts".to_string(),
            text: "api.example.com".to_string(),
            schema: "{\"type\":\"object\"}".to_string(),
        },
    };

    assert_roundtrip(&msg);
}

#[test]
fn client_broadcast_state_update_roundtrip() {
    let msg = ClientBroadcastMessage::StateUpdate(SystemState {
        timestamp: Utc::now(),
        nodes: vec![],
    });

    assert_roundtrip(&msg);
}

#[test]
fn client_direct_intercept_updates_roundtrip() {
    let status = InterceptStatus {
        node_id: "node-1".to_string(),
        enabled: true,
        method: Some(InterceptMethod::Proxy),
        proxy_port: Some(8443),
        intercepted_domains: vec!["api.openai.com".to_string()],
    };

    let msg = ClientDirectMessage::InterceptStatusUpdate(status);
    assert_roundtrip(&msg);
}

#[test]
fn node_direct_registration_ack_roundtrip() {
    let msg = NodeDirectMessage::RegistrationAck(NodeRegistrationAck {
        id: "node-1".to_string(),
        lua_scripts: vec!["print('ok')".to_string()],
    });

    assert_roundtrip(&msg);
}

#[test]
fn intercepted_traffic_roundtrip() {
    let mut req_headers = IndexMap::new();
    req_headers.insert("Authorization".to_string(), "Bearer token-1".to_string());

    let mut resp_headers = IndexMap::new();
    resp_headers.insert("content-type".to_string(), "application/json".to_string());

    let entry = InterceptedTrafficEntry {
        id: Some(1),
        timestamp: Utc::now(),
        node_id: "node-1".to_string(),
        agent_short_name: "claudecode".to_string(),
        intercept_method: InterceptMethod::Proxy,
        direction: TrafficDirection::Send,
        method: Some("POST".to_string()),
        url: "https://api.example.com/v1/chat".to_string(),
        host: "api.example.com".to_string(),
        request_headers: Some(req_headers),
        request_body: Some(br#"{"prompt":"hi"}"#.to_vec()),
        response_status: Some(200),
        response_headers: Some(resp_headers),
        response_body: Some(br#"{"ok":true}"#.to_vec()),
    };

    assert_roundtrip(&entry);
}

#[test]
fn serde_contract_renames_are_stable() {
    let direction = serde_json::to_value(TrafficDirection::Send).expect("serialize direction");
    assert_eq!(direction, json!("send"));

    let scope = RuleScope::Agent {
        node_id: "node-1".to_string(),
        agent_short_name: "codex".to_string(),
    };
    let scope_json = serde_json::to_value(scope).expect("serialize scope");
    assert_eq!(
        scope_json,
        json!({
            "agent": {
                "node_id": "node-1",
                "agent_short_name": "codex"
            }
        })
    );
}

#[test]
fn intercept_rule_roundtrip() {
    let rule = InterceptRule {
        id: 5,
        name: "OpenAI requests".to_string(),
        regex_pattern: "api\\.openai\\.com".to_string(),
        target_direction: TargetDirection::Both,
        scope: RuleScope::All,
        enabled: true,
        summarization_prompt: Some("Summarize security-relevant details".to_string()),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    assert_roundtrip(&rule);
}
