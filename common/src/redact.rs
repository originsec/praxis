use crate::{CredentialItem, ReconResult};

pub const REDACTED: &str = "[REDACTED]";

//
// Key suffixes that name secret material. Keys are compared after
// lower-casing and stripping `_`, `-` and `.`, so `refresh_token`,
// `refreshToken` and `tokens.refresh-token` all normalise to a match.
// Presence flags are named `<field>_present` and so never end in a
// secret suffix.
//

const SECRET_KEY_SUFFIXES: &[&str] = &[
    "token",
    "tokens",
    "secret",
    "password",
    "passwd",
    "apikey",
    "key",
    "cookie",
    "credential",
    "credentials",
    "authorization",
    "bearer",
    "jwt",
];

//
// Well-known secret value prefixes (provider API keys, OAuth tokens).
//

const SECRET_VALUE_PREFIXES: &[&str] = &[
    "sk-",
    "sk_",
    "pk_",
    "rk_",
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
    "github_pat_",
    "glpat-",
    "xox",
    "AIza",
    "ya29.",
    "1//",
    "eyJ",
    "fk-",
    "Bearer ",
];

pub fn is_secret_key(key: &str) -> bool {
    let normalised: String = key
        .chars()
        .filter(|c| !matches!(c, '_' | '-' | '.'))
        .flat_map(char::to_lowercase)
        .collect();
    SECRET_KEY_SUFFIXES
        .iter()
        .any(|suffix| normalised.ends_with(suffix))
}

fn is_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok()
}

fn is_timestamp(value: &str) -> bool {
    chrono::DateTime::parse_from_rfc3339(value).is_ok()
        || chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f").is_ok()
}

//
// Values are treated as secret when they carry a known secret prefix or
// look like an opaque token: long, no whitespace, containing a digit, and
// not one of the structured forms metadata legitimately holds (UUID
// account ids, timestamps, emails, URLs, filesystem paths, comma lists,
// env var names). Random base64/hex of this length virtually always
// contains a digit.
//

pub fn looks_like_secret(value: &str) -> bool {
    let v = value.trim();
    if SECRET_VALUE_PREFIXES.iter().any(|p| v.starts_with(p)) {
        return true;
    }
    if v.len() < 32
        || v.chars().any(char::is_whitespace)
        || !v.chars().any(|c| c.is_ascii_digit())
        || v.contains(',')
    {
        return false;
    }
    if is_uuid(v) || is_timestamp(v) || v.contains('@') || v.contains("://") {
        return false;
    }
    if v.starts_with('/') || v.starts_with('~') || v.get(1..3) == Some(":\\") {
        return false;
    }
    true
}

impl CredentialItem {
    /// Drop secret-named metadata keys and mask secret-looking values.
    pub fn redact(&mut self) {
        self.metadata.retain(|k, _| !is_secret_key(k));
        for value in self.metadata.values_mut() {
            if looks_like_secret(value) {
                *value = REDACTED.to_string();
            }
        }
        if looks_like_secret(&self.path) {
            self.path = REDACTED.to_string();
        }
    }
}

impl ReconResult {
    /// Scrub credential metadata before a recon result leaves the node.
    pub fn redact_secrets(&mut self) {
        for cred in &mut self.config.credentials {
            cred.redact();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn item(metadata: &[(&str, &str)]) -> CredentialItem {
        CredentialItem {
            path: "/home/u/.codex/auth.json".into(),
            kind: "oauth_token_file".into(),
            exists: Some(true),
            last_modified: None,
            metadata: metadata
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<BTreeMap<_, _>>(),
        }
    }

    #[test]
    fn secret_keys_are_dropped() {
        let mut c = item(&[
            ("access_token", "abc"),
            ("refreshToken", "abc"),
            ("tokens.id_token", "abc"),
            ("OPENAI_API_KEY", "abc"),
            ("client_secret", "abc"),
            ("primaryApiKey", "abc"),
            ("email", "user@example.com"),
            ("refresh_token_present", "true"),
        ]);
        c.redact();
        let keys: Vec<_> = c.metadata.keys().cloned().collect();
        assert_eq!(keys, vec!["email", "refresh_token_present"]);
    }

    #[test]
    fn secret_values_are_masked() {
        let mut c = item(&[
            ("note", "sk-ant-api03-abcdefghijklmnop"),
            ("jwt_like", "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.sig"),
            (
                "opaque",
                "Zm9vYmFyYmF6cXV4cXV1eGNvcmdlZ3JhdWx0Z2FycGx5d2FsZG8",
            ),
            ("account_id", "3f2b8c1e-0d4a-4b6f-9a51-6f0e2d7c9b10"),
            ("expires_at", "2026-10-18T12:00:00+00:00"),
            ("email", "someone.with.a.long.name@example-company.com"),
            ("plan", "max"),
            ("used_when", "GEMINI_FORCE_ENCRYPTED_FILE_STORAGE"),
            ("providers", "anthropic,openai-codex,openrouter,fireworks2"),
        ]);
        c.redact();
        assert_eq!(c.metadata["note"], REDACTED);
        assert_eq!(c.metadata["jwt_like"], REDACTED);
        assert_eq!(c.metadata["opaque"], REDACTED);
        assert_eq!(
            c.metadata["account_id"],
            "3f2b8c1e-0d4a-4b6f-9a51-6f0e2d7c9b10"
        );
        assert_eq!(c.metadata["expires_at"], "2026-10-18T12:00:00+00:00");
        assert_eq!(c.metadata["plan"], "max");
        assert_eq!(
            c.metadata["used_when"],
            "GEMINI_FORCE_ENCRYPTED_FILE_STORAGE"
        );
        assert!(c.metadata["providers"].starts_with("anthropic"));
        assert!(c.metadata["email"].contains('@'));
    }

    #[test]
    fn recon_result_redacts_all_credentials() {
        let mut r = ReconResult::default();
        r.config
            .credentials
            .push(item(&[("api_key", "sk-live-123")]));
        r.config
            .credentials
            .push(item(&[("session", "ghp_abcdefghijklmnopqrstuvwxyz0123")]));
        r.redact_secrets();
        let json = serde_json::to_string(&r).unwrap();
        assert!(!json.contains("sk-live-123"));
        assert!(!json.contains("ghp_abcdefghijklmnopqrstuvwxyz0123"));
    }
}
