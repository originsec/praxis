---
name: parallel-fanout
description: Run the same prompt against many nodes and agents at once by creating a temporary operation, queueing it everywhere, collecting the results and cleaning up. Use when the Operator wants one ad-hoc task executed across the fleet.
license: Apache-2.0
metadata:
  author: praxis
  version: "1"
---

# Parallel fan-out with a temporary operation

Operations on different nodes run concurrently, so a temporary operation
is the fastest way to execute one ad-hoc prompt across the fleet.

## Before starting

- Check `op_available` first: if an existing operation already does the
  job, run that instead and skip creation and cleanup.
- Confirm the target set with the Operator (default: every available
  agent on every online node). Use `node_list` / `agent_list` to resolve
  exact agent short names — never guess them.
- Prefer `mode: "one-shot"` unless the task genuinely needs iteration.

## Steps

1. `report_plan` with: create op, queue runs, wait, collect, clean up.
2. `op_create` with a clearly temporary name (category `tmp`, short name
   describing the task). This needs Operator approval.
3. `op_run` once per node/agent target — emit all calls in one response
   so they are queued together.
4. Poll with `op_info` for each returned id. Use `wait` with increasing
   intervals (1s, 2s, 5s, 10s, 15s) between polls instead of tight loops.
5. When every run is finished (or failed after one retry), collect the
   results.
6. `op_delete` the temporary operation (needs Operator approval). Always
   clean up, even if some runs failed.

## Output

A per-target table (node, agent, status, one-line result) plus a short
synthesis of what the results mean. Mention any targets that failed and
why.
