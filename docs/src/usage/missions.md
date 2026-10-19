# Durable Missions

Missions run the Orchestrator as service-owned work. They retain an objective,
operator scope, structured checkpoint, model history, budgets, tool evidence,
and an independent verification result in the service database. Closing the TUI,
detaching a watcher, or disconnecting the CLI does not stop a mission.

Configure the Orchestrator model and enable the MCP server before starting.
Missions use the same configured providers and tools as ordinary Orchestrator
chat. They do not recursively create other missions.

## Missions, operations and chains

A **semantic operation** defines one task to run against an agent. A **chain**
defines a workflow of operations and other blocks with explicit dependencies,
branches and loops. A **mission** owns a persistent objective: the Orchestrator
chooses steps, can invoke operations or chains, collects evidence, and tracks
budgets and checkpoints. Missions are the layer for supervising ongoing work;
chains remain the layer for repeatable, explicitly designed workflows.

## Start and inspect

```bash
praxis mission create inventory "Inventory connected agents and report their versions" \
  --scope "Read-only discovery of connected lab nodes" \
  --max-iterations 30 --max-tokens 200000 --max-active-seconds 1800
praxis mission list
praxis mission show <id>
praxis mission watch <id>
praxis mission events <id> --after 0 --json
```

Use a full ID or an unambiguous prefix of at least four characters. Lists return
up to 1,000 recent missions; older missions remain accessible by full ID. Events
are returned in pages of up to 500; pass the last sequence as `--after` to fetch
the next page. Watch drains pages automatically. `--json` provides structured
responses; watch produces newline-delimited JSON. Ctrl+C while watching detaches.

The TUI has a dedicated **Missions** workspace (`Ctrl+B` or `F6`) with a live
list, creation and lifecycle forms, and Overview/Evidence/Events tabs. It also
accepts the same commands in the Orchestrator input with a `/mission` prefix. `/mission attach <id>` follows events and `/mission detach` stops watching.
Creation and editing show name, objective and scope first; `F4` reveals model,
budget and verification settings. Paused work without uncertain actions resumes
with `r` then `Ctrl+S`. Budget-stopped work opens on the exhausted limit.
Evidence and Events show short previews so large mission journals remain quick
to scroll. Use `praxis mission events <id> --after <sequence> --json` for complete
event payloads.
The MCP `mission` tool exposes the same lifecycle. See [CLI](cli.md),
[TUI](tui.md), and [MCP](mcp.md) for their controls.

Pause a mission before changing its definition. `update` accepts only the fields
you supply, so the other fields remain unchanged:

```bash
praxis mission pause <id>
praxis mission update <id> --objective "Inspect the new target" --scope "Local lab only"
praxis mission update <id> --model provider-model --verify
praxis mission update <id> --clear-model --no-verify
praxis mission resume <id>
```

You can also edit its name or any individual total budget limit with
`--name`, `--max-iterations`, `--max-tokens`, or `--max-active-seconds`.
Existing usage counts and evidence stay intact. A total budget cannot fall
below usage already recorded. `update` works for paused, interrupted,
needs-review, failed, and budget-exceeded missions. Running missions must be
paused first; completed and cancelled missions cannot be edited. The same
command is available in the Orchestrator as `/mission update`, and through the
MCP `mission` action `update` with an `id` and `changes` object.

## Steering, pause, cancellation, and resume

```bash
praxis mission steer <id> "Focus the report on Codex; preserve the earlier findings"
praxis mission pause <id>
praxis mission resume <id>
praxis mission cancel <id>
```

Steering is persisted and incorporated before the next model request. Pause and
cancel signal the worker immediately when given its full ID, including during
stalled model streams or tool calls. Prefixes require a database lookup first. A stop response may still show `running` while the worker records its
final checkpoint; watch or show again to see the settled state. Pause is
resumable; cancellation is terminal. Completed missions are also terminal.

Cancellation attempts to stop known child session prompts and operations.
Cancellation additionally closes sessions created by that mission. These are
bounded, best-effort cleanup requests recorded as events: stopping the
Orchestrator does not roll back external side effects or prove that a remote
process stopped. An action whose response was lost remains uncertain.

After a service restart, previously running missions become `interrupted`,
unless a persisted stop request makes them `paused` or `cancelled`.
Nothing automatically resends their prompt or repeats their tool calls. Completed
tool results are recovered from the event journal; calls never dispatched are
marked as unexecuted. For dispatched calls without durable results, inspect the
endpoint and provide an explicit reconciliation note:

```bash
praxis mission resume <id> \
  --reconciliation "The file was written successfully; do not write it again. Continue with verification."
```

The note enters the resumed context as an operator statement, not as an observed
tool result. It does not automatically replay any action. Checkpoints and history
are saved before dispatch, and each tool result is journaled as it arrives.

## Checkpoints and evidence

The `mission_checkpoint` tool records a summary, findings, outstanding work, and
evidence IDs. Evidence references must identify successful tool results from the
same mission. Planning, waiting, and checkpoint calls are not evidence of an
external outcome. Original tool results remain in the event journal when model
context is compacted.

The verifier receives a service-recorded receipt for each successful checkpoint
call. It proves that the checkpoint was saved, while the findings inside the
checkpoint still require separate observed evidence.

Context compaction retains the objective, pinned checkpoint, recent complete tool
exchanges, and bounded extracts from older context. Extracts are explicitly
incomplete. Token estimates are conservative byte-based estimates, not a
provider-specific tokenizer. Oversized pinned context fails explicitly rather
than silently exceeding the configured context bound.

Mission tool calls execute in order to avoid overlapping mutations of a shared
session. Independent workflow branches execute concurrently through the bounded
[chain scheduler](semantic-operations.md#chain-execution). Queuing an asynchronous
operation and awaiting its outcome are separate actions.

## Budgets

Budgets cover cumulative model iterations, tokens, and active time across all
resumes, including the verification pass. The default is 50 iterations, 500,000
tokens, and 3,600 active seconds. These are limits for the mission's own model
requests, not a dollar budget or an accounting of target-agent/provider costs.

Before inference, the service reserves estimated input tokens plus the requested
maximum output. A completed response replaces that reservation with reported
usage (or an estimate when unavailable). Interrupted requests keep their
reservation because actual usage is unknown. A single provider response may
exceed estimates; no further inference is started after the total is exhausted.

Active time excludes deliberate pauses. After an unclean restart the elapsed
wall time since the persisted run start is charged conservatively, including
unknown downtime. To resume after exhaustion, raise the exhausted **total** limit:

```bash
praxis mission resume <id> --max-tokens 800000
```

## Verification and status

By default, the investigator's report is reviewed in a separate model context
with no executable tools. The verifier receives the objective, scope, report,
checkpoint, and recorded tool evidence. Checkpoint evidence is included first,
with remaining results ordered deterministically and omissions disclosed.
Completion requires a positive verdict referencing evidence shown to the verifier. Otherwise the mission becomes `needs_review` and can be
steered and resumed. `--no-verify` explicitly disables this review.

This is an independent context using the selected model, not a deterministic
proof or an independent endpoint probe. Provider/tool output can be incomplete
or misleading; the verdict and source evidence remain inspectable. Evidence
included in the verifier context is bounded, so a large investigation can require
operator review.

Other states are `running`, `paused`, `interrupted`, `completed`, `cancelled`,
`failed`, and `budget_exceeded`. Failures retain the checkpoint and error for
inspection and explicit resumption.

Scope is an instruction to the agent. It does not add an OS sandbox, replace
existing endpoint permissions, or introduce per-user authorization. Mission
records and events use the service's existing trusted deployment model. Stored
content has no automatic retention policy in this version.
