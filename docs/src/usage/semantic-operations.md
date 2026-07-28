# Semantic Operations

Semantic operations are predefined tasks that run through AI agents. You define what you want to happen in natural language, and Praxis handles the execution.

## What's a Semantic Operation?

An operation is a task specification:

- **Name** - Identifier for the operation
- **Prompt** - What you want the agent to do
- **Mode** - How to execute (one-shot or agent)
- **Timeout** - How long to wait
- **Max Iterations** - Cap on orchestrator/target-agent turns in agent mode (default 5)
- **YOLO Mode** - Auto-approve actions

Think of operations as reusable prompts with execution settings.

## Execution Modes

### One-Shot Mode

Sends a single prompt to the agent and waits for a response.

How it works:
1. Create a session (if needed)
2. Send the operation prompt
3. Wait for the agent to respond
4. Return the response
5. Close the session (if we created it)

Best for: Simple tasks, single actions, quick checks.

### Agent Mode

Uses an orchestrating LLM to run multi-turn interactions with the target agent.

How it works:
1. Orchestrator LLM receives the operation prompt
2. Orchestrator generates a prompt for the target agent
3. Target agent responds
4. Orchestrator evaluates and decides next action
5. Loop continues until complete or max iterations reached

Best for: Complex tasks, multi-step operations, tasks requiring judgment.

The orchestrator is a separate LLM (configured in Settings as "Semantic Ops" LLM) that manages the interaction. It has access to a `session_prompt` tool to communicate with the target agent.

### Model Requirements

Agent mode requires a sufficiently capable model for the orchestrator. The model must be able to:
- Follow complex multi-step instructions
- Output tool calls in the correct JSON format
- Wait for tool results before proceeding
- Avoid hallucinating results

**Recommended models:**
- Anthropic: Claude Sonnet 4 or Claude Opus 4
- OpenAI: GPT-4o or GPT-4 Turbo
- Google: Gemini 1.5 Pro

**Not recommended for agent mode:**
- Smaller/faster models (Haiku, GPT-4o-mini, Llama 8B) - these often fail to follow tool calling instructions correctly and may hallucinate results
- Models without strong instruction-following capabilities

If you're seeing issues with tool calling or hallucinated results, try switching to a more capable model.

### Agent Mode Architecture

The orchestrator uses a system prompt that defines its behavior:

**Prompt Location**: `service/src/prompts/semantic_op_agent.prompt`

The system prompt is embedded at build time using Rust's `include_str!` macro. This means:
- Prompts are part of the compiled binary
- No runtime configuration of prompts is needed or supported
- Changes require recompilation

The orchestrator prompt is combined with:
- Tool calling instructions (`common/src/prompts/tool_calling.prompt`)
- Task completion instructions (`common/src/prompts/task_completion.prompt`)

These define the JSON format the orchestrator uses to call tools and signal completion:

```json
{"tool": "session_prompt", "args": {"text": "..."}}
```

```json
{"complete": true, "summary": "...", "result": true}
```

`result` is a boolean success flag, not a text field — put findings, extracted data, or other output in `summary` instead.

## Creating Operations

Operations are stored in the library:

1. Open **Operations** with `Ctrl+P` and select the **Library** tab
2. Press `Ctrl+N` to open the operation form
3. Fill in the details:
   - Name and description
   - Operation prompt
   - Mode (one-shot or agent)
   - Timeout value
   - Max iterations (agent mode)
   - YOLO mode setting
4. Save with `Ctrl+S`

Operations are stored in the database and available across sessions.
See [Terminal UI](./tui.md#operations-ctrlp) for the complete Operations
controls.

## Running Operations

### From the Library

1. Open **Operations** with `Ctrl+P` and select the **Library** tab
2. Select the operation and press `Ctrl+R`
3. Select the node and agent in the run form
4. Watch execution in the **Executions** tab

## Monitoring Execution

The **Executions** tab shows all running and completed operations:

| Column | Description |
|--------|-------------|
| Name | Operation being executed |
| Node/Agent | Where it's running |
| Status | Running, Completed, Failed, Cancelled |
| Started | When execution began |

Select a run and press `Enter` to focus its details:
- Full execution output
- Iteration history (agent mode)
- Final result or error

## Operation Output

Each operation produces output:

**One-shot mode** - The agent's response to your prompt.

**Agent mode** - Full transcript of the orchestrator's iterations:
- Prompts sent to target agent
- Responses received
- Orchestrator's reasoning
- Final result

## Built-in Operations

Praxis comes with some predefined operations for common tasks. You can use these as-is or as templates for your own.

## YOLO Mode in Operations

When YOLO mode is enabled for an operation:
- The target agent session is created with auto-approve
- Actions execute without user confirmation
- The entire operation runs hands-off

This is useful for automated scenarios but removes safety checks.

## Model Override

Operations can specify a different model than the default:
- Override the Semantic Ops LLM for specific operations
- Use faster models for simple operations
- Use more capable models for complex tasks

## Cancellation

Running operations can be cancelled:
1. Open **Operations** with `Ctrl+P` and select the **Executions** tab
2. Select the running operation and press `Ctrl+C`
3. The operation terminates

Cancellation is best-effort-if the agent is mid-action, that action may complete.

## Timeouts

Each operation has a timeout:
- One-shot: Time to wait for agent response
- Agent mode: Total time for all iterations

When timeout is reached, the operation fails with a timeout error.

## Chaining Operations

Operations can be combined into chains for complex workflows. A chain is a graph of operations with connections defining execution order and session groups controlling how sessions are shared.

### Visual Chain Builder

The shipped chain builder is part of the **praxis TUI**. Open
**Operations** with `Ctrl+P`, select **Library**, then use `Ctrl+Alt+N` to
create a chain or `Ctrl+E` to edit the selected chain. It is a terminal
canvas with blocks, connections, and properties.

See [TUI — chain builder](tui.md#library-tab--chain-builder) for the
canonical keyboard and mouse controls.

### Chain Structure

Every chain starts with a **Trigger** element and must include exactly one **Termination**. Between them, you build processing workflows using various block types. Incomplete blocks show a `!` badge on the canvas.

### Element Types

Chains support several element types:

**Trigger** - Every chain must start with a trigger. The in-canvas trigger
element represents manual execution from the Operations library. For automated
triggers, see [Chain Triggers](#chain-triggers) below.

**Operation** - Executes a semantic operation from your library. Pick an existing operation by name (picker opens when you add an Operation). The operation runs against the target agent and its output flows to the next element.

**Transform** - An LLM-powered transformation step. Takes input from the previous element and applies a multi-line prompt to transform it. Useful for extracting specific data, reformatting output, or summarizing information.

**GenericPrompt** - Sends a multi-line prompt directly to the agent session (not through an orchestrator). Simpler than an operation — just sends the prompt and captures the response.

**Memory** - Stores or retrieves data by key (mode is store or retrieve). Store passes data through unchanged; retrieve loads a previously stored key.

**Loop** - Controls retries. `max_iterations` is the number of times the
retry path may run: port `r` (0) fires on each retry and port `x` (1) fires
once the retries are exhausted. For example, with `max_iterations: 1`, the
Loop takes `r` once and then takes `x` on its next visit.

**Tool** - Invokes a registered toolkit tool (picker lists known tools; params are JSON).

**Payload** - Emits a stored payload by id.

**Termination** - Explicit end of the chain (exactly one per chain).

### Conditional Connections

Connections between elements can have conditions:

- **Always** (default) - The connection always fires when the source completes
- **On Success** - Fires only when the source element completes successfully
- **On Failure** - Fires only when the source element fails

This enables branching workflows with error handling paths.

### Per-Block Configuration

Operation, Transform, and GenericPrompt elements can carry per-block configuration overrides. Of these, only two currently take effect at execution time:
- **YOLO Mode** - Enable auto-approve for this element's session
- **Require All Inputs** - When disabled, a merge-point element runs as soon as any upstream input arrives (instead of waiting for all branches). Useful in conditional chains where not all paths execute.

Two more fields are exposed in the properties modal and stored with the block, but are not yet read by the executor:
- **Max Runtime** - Intended as a timeout in seconds for this specific element
- **Working Directory** - Intended to override the working directory

Setting either of the last two currently has no effect on execution.

### Building a Chain

New chains open with a connected `Trigger → Termination` scaffold. Add the
processing elements required for the workflow, keep exactly one Termination,
and configure the selected element from its properties. Operations can select
an operation and optional model; transforms and generic prompts use a prompt;
memory, loops, tools, and payloads each expose their relevant fields.

Use the session-group picker on Operations, Transforms, and GenericPrompts to
create, reuse, or omit a group. Elements in the same group share a color tick
and agent-session context. The [TUI chain-builder guide](tui.md#library-tab--chain-builder)
contains the interaction details and complete shortcuts.

### Session Groups

Session groups control how agent sessions are managed across chain elements. Elements that interact with agents (Operations, Transforms, GenericPrompts) can be assigned to session groups.

**Assigning Session Groups:** Open an element's properties, choose a session
group, then select none, create a new group, or reuse an existing group id.
Elements in the same group share a color indicator on the canvas.

**Same Session Group** - Elements share an agent session:
- The first element creates the session
- Subsequent elements reuse it
- Session closes after the last element completes
- Context and state persist between elements

**Different Session Groups** - Elements get isolated sessions:
- Each group has its own session
- Clean separation, no shared context
- Useful for independent operations

**No Session Group** - Element gets a fresh session just for itself.

**Why Session Groups Matter:**

Agent sessions maintain conversation context. If you run an operation that navigates to a directory, the next operation in the same session starts in that directory. Use session groups when:
- Operations build on each other's state
- You want to maintain conversation context
- Sequential steps depend on previous actions

Use separate groups when:
- Operations should be isolated
- You want clean slate for each operation
- Running parallel independent tasks

### Chain Execution

When running a chain:

1. The executor builds a dependency graph from connections
2. Finds operations with no dependencies (starting points)
3. Works through ready elements one at a time from a queue, fully awaiting each before starting the next
4. As each element completes, marks it done and enqueues any newly-ready elements
5. Repeats until all complete, or the chain ends via Termination

Elements without dependencies on each other become ready at the same time, but execution is currently sequential — the executor does not run them concurrently, so ordering among independently-ready elements depends on queue order rather than true parallelism. Whether a failed element halts downstream progress depends on how its outgoing connections are configured — see Conditional Connections below.

```diagram
    ┌─────┐
    │Start│
    └──┬──┘
       │
   ┌───┴───┐
   │       │
┌──▼──┐ ┌──▼──┐
│Op A │ │Op B │  ← Both become ready together; run one at a time, not concurrently
└──┬──┘ └──┬──┘
   │       │
   └───┬───┘
       │
    ┌──▼──┐
    │Op C │  ← This waits for both A and B
    └─────┘
```

### Monitoring Chains

Chain executions appear in the **Executions** tab alongside individual
operations. Select one and press `Enter` to see element status, output, and
timing information.

### Chain Cancellation

You can cancel a running chain from the **Executions** tab with `Ctrl+C`.
Cancellation stops queuing new operations and lets running operations complete
(or cancels them).

### Use Cases

**Sequential Operations** - Run operations in order, each building on the previous: enumerate capabilities, identify target, execute action, verify result.

**Parallel Reconnaissance** - Run multiple recon operations simultaneously, then combine results.

**Staged Operations** - Build up context across operations with shared sessions, maintaining state throughout.

### Chain Best Practices

- Plan session groups carefully - shared sessions maintain context but accumulate state
- Handle failures - by default a connection fires regardless of success or failure (see Conditional Connections); use On Success/On Failure routing if you need the chain to stop or branch on failure
- Test incrementally - run individual operations first, then combine
- Keep chains focused - one chain, one goal

### Chain Triggers

Chains can be executed automatically via triggers. While the in-canvas Trigger
element represents manual execution, chain triggers are separate
configurations that automate when and how a chain fires. Manage them from
**Operations** (`Ctrl+P`) → **Triggers**, not inside the chain canvas.

#### Trigger Types

**Scheduled** - Fires on a time-based schedule. Two schedule modes are available:

- **Interval** - Fires every N minutes (e.g., every 60 minutes). The next fire time is computed from the last fire time.
- **Daily At** - Fires once per day at a specific hour and minute (UTC). If the time has already passed today, the next fire is scheduled for tomorrow.

Scheduled triggers can be **recurring** (fire repeatedly) or **one-shot** (fire once and then auto-disable).

**Intercept Match** - Fires when intercepted traffic matches a specific intercept rule. You specify the rule ID, and whenever traffic triggers that rule, the chain executes. Intercept-match triggers have a 60-second debounce window to prevent rapid repeated firings.

**New Node** - Fires whenever a new node registers with the service. There is a 10-second delay after registration to allow agent discovery to complete before the chain executes.

#### Creating Triggers

From **Operations** (`Ctrl+P`) → **Triggers**:

1. Press `Ctrl+N` to open the trigger form
2. Select the target chain, trigger type, and schedule/rule settings
3. Configure the **Target Spec** (see [Flexible Targeting](#flexible-targeting) below)
4. Save with `Ctrl+S`

The trigger is immediately active once saved. Each chain can have multiple triggers.

#### Managing Triggers

The **Triggers** tab shows all configured triggers across all chains. From
there you can:

- See the chain name, trigger type, configuration summary, and target spec for each trigger
- Toggle the selected trigger with `Enter`
- View when a trigger last fired and when it will next fire
- Edit or delete the selected trigger with the Operations controls documented in
  [Terminal UI](./tui.md#operations-ctrlp)

#### Trigger Engine

The service runs a trigger engine that polls for due scheduled triggers every 30 seconds. When a trigger fires:

1. The engine loads the chain definition
2. Resolves the target spec into concrete node/agent pairs
3. Executes the chain against each resolved target (fan-out)
4. Updates the trigger's `last_fired_at` timestamp
5. For scheduled triggers, computes the next fire time (or disables if non-recurring)

Event-based triggers (Intercept Match, New Node) fire immediately in response to the event rather than on a polling schedule.

### Flexible Targeting

By default, chains run against a single node and agent. The **TargetSpec** system allows chains to target multiple nodes and agents simultaneously using filters.

#### Target Spec Fields

| Field | Description | Default |
|-------|-------------|---------|
| **Node IDs** | Specific node IDs to target | Empty (all nodes) |
| **OS Filter** | Case-insensitive substring match on the node's OS details | None |
| **Agent Short Names** | Specific agent types to target | Empty (all available agents) |
| **Include Triggering Node** | For event triggers: ensure the node that caused the event is included | Off |

When a trigger fires, the target spec is resolved against the current set of registered nodes:

1. Start with all registered nodes
2. Filter by specific node IDs (if any specified)
3. Filter by OS substring (if specified)
4. For each remaining node, select agents matching the agent filter
5. Skip agents that are not currently available

If no targets match, the trigger logs a warning and the chain does not execute.

#### Target Spec Editor

The target-spec editor appears when creating triggers in the chain builder and
when using advanced targeting in the run form. It provides:

- **Node multi-select** - Pick specific nodes from the connected nodes list, or leave empty for all nodes
- **OS filter** - Free text field for OS substring matching (e.g., "Windows", "Linux", "Ubuntu")
- **Agent multi-select** - Pick specific agent types, or leave empty for all available agents
- **Include triggering node** - Checkbox shown for event triggers (New Node, Intercept Match) to ensure the triggering node is always included even if it would otherwise be filtered out

#### Fan-Out Execution

When a chain targets multiple node/agent pairs, the executor performs a
fan-out: it creates a separate chain execution for each resolved target. Each
execution runs independently and appears in the **Executions** tab.

#### Advanced Targeting in Run Form

The chain run form includes an **Advanced Targeting** toggle. When enabled,
instead of selecting a single node and agent, you configure a full target spec.
This allows manual one-off fan-out runs without needing to set up a trigger.

## Troubleshooting

### Operation stuck

- Check if YOLO mode should be enabled
- Verify the agent session is responsive
- Try a simpler prompt

### Unexpected results

- Review the full output
- Check if the prompt is clear enough
- Consider using agent mode for complex tasks

### Timeouts

- Increase the timeout value
- Simplify the operation
- Check if the agent is responding at all

### Tool calling not working (agent mode)

Symptoms: The orchestrator outputs tool calls but they don't execute, or execution completes immediately without actually running the tool.

- **Switch to a more capable model** - smaller models often fail to follow the tool calling format correctly. Use Claude Sonnet/Opus, GPT-4o, or Gemini 1.5 Pro
- Check the operation output for malformed JSON in tool calls
- Verify the model is outputting the correct format: `{"tool": "session_prompt", "args": {"text": "..."}}`

### Hallucinated or fabricated results

Symptoms: The operation completes with results that look plausible but are entirely made up - the orchestrator never actually called the remote agent.

This happens when a model outputs both a tool call AND a completion signal in the same message, fabricating results instead of waiting for the real tool response.

- **Use a more capable model** - this is almost always caused by using a model that doesn't follow instructions well
- Check the full operation output - if you see a tool call immediately followed by a completion signal with results, the model hallucinated
- Recommended: Claude Sonnet 4+, GPT-4o, or Gemini 1.5 Pro
- Avoid: Smaller/faster models like Haiku, GPT-4o-mini, or small open-source models for agent mode orchestration
