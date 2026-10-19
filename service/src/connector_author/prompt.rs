use std::fmt::Write;

use common::connector_author::{CommandCapture, ConnectorDryRun, ConnectorProbe};

const SYSTEM_PROMPT: &str = include_str!("../prompts/connector_author.prompt");
const HELPERS_LUA: &str = include_str!("../../../node/src/agent_connectors/lua/lib/helpers.lua");
const AUTHORING_GUIDE: &str = "connectors/adding-new.md";
const GUIDE_SECTIONS: &[&str] = &["## Lua Connector (Recommended)", "## ACP Sessions"];

//
// Example connectors shown to the model: a subprocess-based CLI connector,
// plus an ACP-based one when the probe saw an ACP mode.
//

const SUBPROCESS_EXAMPLE: &str = "droid";
const ACP_EXAMPLE: &str = "gemini";

pub(super) fn system_prompt() -> String {
    SYSTEM_PROMPT.to_string()
}

pub(super) fn generate_prompt(probe: &ConnectorProbe) -> String {
    let mut out = reference_material(probe.acp_advertised);
    out.push_str(
        "# Task\n\nWrite a new Praxis Lua connector for the agent described by this probe.\n\n",
    );
    out.push_str(&render_probe(probe));
    out
}

pub(super) fn repair_prompt(current_script: &str, failure: &str, probe: &ConnectorProbe) -> String {
    let mut out = reference_material(probe.acp_advertised);
    out.push_str("# Task\n\nThe connector below has stopped working, most likely because the agent was updated. Fix it. Keep its name, short_name and overall structure; change only what the failure and the fresh probe require.\n\n");
    let _ = write!(
        out,
        "## Current connector\n\n```lua\n{}\n```\n\n## Failure\n\n```\n{}\n```\n\n",
        current_script.trim_end(),
        failure.trim_end()
    );
    out.push_str(&render_probe(probe));
    out
}

pub(super) fn feedback_prompt(validation: &ConnectorDryRun) -> String {
    format!(
        "The dry run on the node failed:\n\n```\n{}\n```\n\nFix every reported problem and reply with the complete corrected script in one ```lua block.",
        validation.report()
    )
}

fn reference_material(include_acp_example: bool) -> String {
    let mut out = String::from("# Reference: connector authoring guide\n\n");
    let guide =
        crate::doc_helper::retrieval::read_page(AUTHORING_GUIDE, 200_000).unwrap_or_default();
    for heading in GUIDE_SECTIONS {
        if let Some(section) = markdown_section(&guide, heading) {
            out.push_str(section.trim_end());
            out.push_str("\n\n");
        }
    }

    out.push_str("# Reference: praxis.helpers API (signatures and doc comments)\n\n```lua\n");
    out.push_str(&helper_api_reference(HELPERS_LUA));
    out.push_str("```\n\n");

    let mut examples = vec![SUBPROCESS_EXAMPLE];
    if include_acp_example {
        examples.push(ACP_EXAMPLE);
    }
    for name in examples {
        if let Some((_, content)) = crate::EMBEDDED_LUA_SCRIPTS
            .iter()
            .find(|(stem, _)| *stem == name)
        {
            let _ = write!(
                out,
                "# Example connector: agents/{}.lua\n\n```lua\n{}\n```\n\n",
                name,
                content.trim_end()
            );
        }
    }
    out
}

//
// Text of a level-2 markdown section starting at a heading that begins with
// `heading`, up to (not including) the next level-2 heading. Headings inside
// fenced code blocks are ignored.
//

pub(super) fn markdown_section<'a>(doc: &'a str, heading: &str) -> Option<&'a str> {
    let mut start = None;
    let mut in_fence = false;
    let mut offset = 0;
    for line in doc.split_inclusive('\n') {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
        }
        if !in_fence && line.starts_with("## ") {
            if let Some(s) = start {
                return Some(&doc[s..offset]);
            }
            if line.starts_with(heading) {
                start = Some(offset);
            }
        }
        offset += line.len();
    }
    start.map(|s| &doc[s..])
}

//
// Each `function M.name(...)` signature in helpers.lua together with the
// comment block directly above it, without function bodies.
//

pub(super) fn helper_api_reference(source: &str) -> String {
    let mut out = String::new();
    let mut comment: Vec<&str> = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("--") && !line.starts_with(' ') {
            comment.push(line);
            continue;
        }
        if line.starts_with("function M.") {
            let doc: Vec<&str> = comment
                .iter()
                .copied()
                .filter(|l| l.trim() != "--")
                .collect();
            for l in doc {
                out.push_str(l);
                out.push('\n');
            }
            out.push_str(line);
            out.push_str(" ... end\n\n");
        }
        if !line.trim().is_empty() {
            comment.clear();
        }
    }
    out
}

fn render_capture(out: &mut String, label: &str, capture: &CommandCapture) {
    let status = if capture.timed_out {
        "timed out".to_string()
    } else {
        format!(
            "exit {}",
            capture
                .exit_code
                .map(|c| c.to_string())
                .unwrap_or("?".into())
        )
    };
    let _ = write!(
        out,
        "### `{} {}` ({}{})\n\n```\n{}\n```\n\n",
        label,
        capture.args.join(" "),
        status,
        if capture.truncated { ", truncated" } else { "" },
        capture.output.trim_end()
    );
}

pub(super) fn render_probe(probe: &ConnectorProbe) -> String {
    let mut out = String::new();
    let exe = probe.executable.as_deref().unwrap_or("(not found)");
    let _ = write!(
        out,
        "## Probe of `{}` (node OS: {})\n\nExecutable: `{}`\n\n",
        probe.requested, probe.os, exe
    );
    if let Some(version) = &probe.version {
        render_capture(&mut out, exe, version);
    }
    if let Some(help) = &probe.help {
        render_capture(&mut out, exe, help);
    }
    if probe.acp_advertised {
        let _ = write!(
            out,
            "ACP mode: ADVERTISED. Help lines mentioning it:\n{}\n\n",
            probe
                .acp_evidence
                .iter()
                .map(|l| format!("- `{}`", l))
                .collect::<Vec<_>>()
                .join("\n")
        );
    } else {
        out.push_str("ACP mode: not advertised in the help text.\n\n");
    }

    out.push_str("### Config / data directories (names, sizes and types only)\n\n");
    if probe.dirs.is_empty() {
        out.push_str("None found in the usual locations.\n\n");
    }
    for dir in &probe.dirs {
        let _ = writeln!(
            out,
            "`{}`{}",
            dir.path,
            if dir.truncated {
                " (listing truncated)"
            } else {
                ""
            }
        );
        for entry in &dir.entries {
            let _ = writeln!(
                out,
                "  {:<7} {}{}",
                entry.kind,
                entry.path,
                if entry.kind == "file" {
                    format!("  ({} B)", entry.size)
                } else {
                    String::new()
                }
            );
        }
        out.push('\n');
    }

    if !probe.configs.is_empty() {
        out.push_str("### Config files (secret-looking values were redacted on the node)\n\n");
        for config in &probe.configs {
            let _ = write!(
                out,
                "`{}` ({} redaction(s))\n\n```{}\n{}\n```\n\n",
                config.path,
                config.redactions,
                config.format,
                config.content.trim_end()
            );
        }
    }
    for note in &probe.notes {
        let _ = writeln!(out, "Note: {}", note);
    }
    out
}
