# Semantic Operations

Semantic operations are predefined tasks that run through AI agents. You define what you want to happen in natural language, and Praxis handles the execution.

## What's a Semantic Operation?

An operation is a task specification:

- **Name** - Identifier for the operation
- **Prompt** - What you want the agent to do
- **Mode** - How to execute (one-shot or agent)
- **Timeout** - How long to wait
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
{"complete": true, "summary": "...", "result": "..."}
```

## Creating Operations

Operations are stored in the library:

1. Go to **Operations** → **Library** tab
2. Click **New Operation**
3. Fill in the details:
   - Name and description
   - Operation prompt
   - Mode (one-shot or agent)
   - Timeout value
   - YOLO mode setting
4. Save

Operations are stored in the database and available across sessions.

## Running Operations

### From the Library

1. Go to **Operations** → **Library**
2. Find the operation
3. Click **Run**
4. Select node and agent
5. Watch execution in the Runs tab

### From an Agent

1. Open an agent's detail page
2. Go to the **Ops** tab
3. Click **Run Operation**
4. Select from available operations

## Monitoring Execution

The Runs tab shows all running and completed operations:

| Column | Description |
|--------|-------------|
| Name | Operation being executed |
| Node/Agent | Where it's running |
| Status | Running, Completed, Failed, Cancelled |
| Started | When execution began |

Click a run to see details:
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
1. Find the operation in Runs
2. Click **Cancel**
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

Praxis includes a visual chain builder using React Flow:

1. Go to **Operations** → **Library**
2. Click **New Chain**
3. Drag operations onto the canvas
4. Connect outputs to inputs
5. Configure session groups
6. Save the chain

### Chain Structure

A chain consists of:

- **Start Node** - Entry point, begins execution
- **Operation Nodes** - Each runs a semantic operation
- **Connections** - Define execution order and data flow
- **End** - Implicit when all paths complete

### Session Groups

Session groups control how sessions are managed across operations.

**Same Session Group** - Operations share a session. The first operation creates the session, subsequent operations reuse it, and it closes after the last operation. This maintains context between operations.

**Different Session Groups** - Operations get their own sessions with clean isolation and no shared context.

Why does this matter? Agent sessions have context. If one operation sets something up, the next operation in the same session sees that state.

### Chain Execution

When running a chain:

1. The executor builds a dependency graph from connections
2. Finds operations with no dependencies (starting points)
3. Executes ready operations (possibly in parallel)
4. Marks completed, finds newly ready operations
5. Repeats until all complete or one fails

Operations without dependencies on each other can run simultaneously. The executor identifies these and runs them in parallel.

```
    ┌─────┐
    │Start│
    └──┬──┘
       │
   ┌───┴───┐
   │       │
┌──▼──┐ ┌──▼──┐
│Op A │ │Op B │  ← These run in parallel
└──┬──┘ └──┬──┘
   │       │
   └───┬───┘
       │
    ┌──▼──┐
    │Op C │  ← This waits for both A and B
    └─────┘
```

### Monitoring Chains

Chain executions appear in the Runs tab alongside individual operations. Click a chain execution to see individual element status, output from each operation, and timing information.

### Chain Cancellation

You can cancel a running chain from the Runs tab. Cancellation stops queuing new operations and lets running operations complete (or cancels them).

### Use Cases

**Sequential Operations** - Run operations in order, each building on the previous: enumerate capabilities, identify target, execute action, verify result.

**Parallel Reconnaissance** - Run multiple recon operations simultaneously, then combine results.

**Staged Operations** - Build up context across operations with shared sessions, maintaining state throughout.

### Chain Best Practices

- Plan session groups carefully - shared sessions maintain context but accumulate state
- Handle failures - if an operation fails, the chain stops
- Test incrementally - run individual operations first, then combine
- Keep chains focused - one chain, one goal

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
