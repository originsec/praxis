---
name: fleet-recon
description: Inventory every connected node and its AI agents, run reconnaissance where it is missing, and summarize each agent's tools, MCP servers, configuration and recent sessions. Use when the Operator asks what is out there, wants a fleet overview, or before planning an engagement.
license: Apache-2.0
metadata:
  author: praxis
  version: "1"
---

# Fleet reconnaissance

Builds a current picture of the fleet without changing anything on the
endpoints. All steps are read-only.

## Steps

1. `report_plan` with the steps below.
2. `node_list` to get every connected node (skip nodes that are offline).
3. `agent_list` for each node — emit these calls together so they run
   concurrently.
4. For each node/agent pair, `recon_list` with `section: "all"` to see
   what is already stored. If nothing is stored (or the Operator asked
   for fresh data), run `recon_run` for that pair. Recon on different
   nodes can run in parallel.
5. Use `recon_list` sections (`tools`, `configs`, `sessions`,
   `projects`) and `recon_config_grep` to pull out:
   - MCP servers and tools each agent can use
   - permission / approval settings (auto-approve, yolo, allow-lists)
   - credentials or tokens referenced in config (report the location,
     never print secret values in full)
   - recent session activity and project paths
6. If delegation is available and the fleet is large, `delegate` one
   sub-task per node with the steps above and merge the summaries.
7. Record notable findings with `memory_write` (one memory per node,
   titled `recon: <hostname>`), so later sessions in this engagement can
   reuse them.

## Output

A short summary followed by a table (keep within 80 columns):

| Node | Agent | Tools/MCP | Approval mode | Notes |

Close with the most interesting targets and suggested next steps. Do not
run operations or prompts against agents without the Operator's go-ahead.
