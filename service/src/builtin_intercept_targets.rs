//
// Default intercept target list seeded into the service database on
// first boot (and re-seeded when BUILTIN_INTERCEPT_TARGETS_VERSION
// changes). Built-ins remain editable from the management UI; the
// is_builtin flag only determines whether they are recreated on
// version bumps. User-deleted built-ins stay deleted because the
// version-bump update path keys on existing rows by id.
//

pub struct BuiltinTarget {
    pub id: &'static str,
    pub name: &'static str,
    pub agent_short_name: &'static str,
    pub domains: &'static [&'static str],
    pub url_pattern: Option<&'static str>,
}

//
// Bumped when the BUILTIN_INTERCEPT_TARGETS list below changes in a way
// that should propagate to existing installations. Mirrors the
// builtin_scripts_version key used for Lua agent scripts.
//

pub const BUILTIN_INTERCEPT_TARGETS_VERSION: &str = "1";

pub const BUILTIN_INTERCEPT_TARGETS: &[BuiltinTarget] = &[
    BuiltinTarget {
        id: "builtin-claudecode",
        name: "Claude Code",
        agent_short_name: "claudecode",
        domains: &["api.anthropic.com", "a-api.anthropic.com"],
        url_pattern: Some("messages"),
    },
    BuiltinTarget {
        id: "builtin-claudedesktop",
        name: "Claude Desktop",
        agent_short_name: "claudedesktop",
        domains: &["api.anthropic.com", "a-api.anthropic.com"],
        url_pattern: Some("messages"),
    },
    BuiltinTarget {
        id: "builtin-cursor",
        name: "Cursor Agent",
        agent_short_name: "cursor",
        domains: &["api.cursor.sh", "agent.api5.cursor.sh", "api2.cursor.sh", "cursor.sh"],
        url_pattern: None,
    },
    BuiltinTarget {
        id: "builtin-droid",
        name: "Droid CLI",
        agent_short_name: "droid",
        domains: &[
            "api.factory.ai",
            "staging.api.factory.ai",
            "preprod.api.factory.ai",
            "dev.api.factory.ai",
        ],
        url_pattern: None,
    },
    BuiltinTarget {
        id: "builtin-gemini",
        name: "Gemini CLI",
        agent_short_name: "gemini",
        domains: &["generativelanguage.googleapis.com", "cloudcode-pa.googleapis.com"],
        url_pattern: None,
    },
    BuiltinTarget {
        id: "builtin-m365copilot",
        name: "Microsoft 365 Copilot",
        agent_short_name: "m365copilot",
        domains: &["substrate.office.com"],
        url_pattern: Some("m365Copilot/Chathub"),
    },
];
