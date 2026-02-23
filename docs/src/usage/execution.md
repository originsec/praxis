# Execution

The Execution page provides a unified workspace for building, executing, and monitoring operation chains with an integrated AI assistant.

## Layout

The interface is divided into three main areas:

- **Tab Bar** — Multi-tab chain builder with close, rename, and add controls
- **Main Area** — Chain builder (edit mode) or execution viewer (run mode)
- **Chain Orchestrator Pane** — Collapsible AI assistant panel on the right

## Tabs

Each tab represents an independent workspace that can contain:

- A new unsaved chain being built
- An existing chain loaded for editing
- A running or completed chain execution

Tabs support:

- **Rename**: Double-click the tab name to edit it
- **Close**: Click the X button (at least one tab must remain)
- **Add**: Click the + button to create a new empty tab
- **Dirty indicator**: A yellow dot appears when a tab has unsaved changes
- **Running indicator**: A spinner appears when an execution is active in the tab

## Chain Builder

The chain builder allows visual construction of operation chains using drag-and-drop. All nine element types are supported:

- **Trigger** — Starting point (exactly one per chain)
- **Operation** — Runs a semantic operation definition
- **Transform** — LLM-powered data transformation with a prompt
- **GenericPrompt** — Direct agent session prompt
- **Memory** — Store or retrieve data by key within an execution
- **Loop** — Create iteration loops with configurable max iterations
- **Tool** — Run toolkit tools (e.g., session poisoning, message encoders)
- **Payload** — Inject stored content from the payload database
- **Termination** — Chain output endpoint

Connections support conditional routing with **OnSuccess** and **OnFailure** conditions for error handling and branching logic.

Additional controls in the Execution page:

- **Run**: Execute the chain on a target node and agent
- **Create Op**: Open the inline operation creator to define new operations without leaving the page

### Inline Operation Creator

The slide-out panel lets you create new operation definitions directly from the chain builder. Fields include:

- **Name** and **Category**: Identify the operation
- **Mode**: Single Prompt or Agent (multi-iteration)
- **Operation Prompt**: The instructions for the LLM
- **Model**: Optional model override
- **YOLO Mode**: Enable autonomous tool execution

## Execution Viewer

When a chain is running, the tab switches to the execution viewer showing:

- **Flow Graph**: Visual chain with status indicators on each element (pending, running, completed, failed)
- **Event Timeline**: Granular real-time events including prompts sent, responses received, tool calls, agent iterations, LLM calls, and session lifecycle
- **Outputs**: Final chain outputs displayed when execution completes
- **Cancel**: Stop a running execution

Click on a node in the flow graph to filter the event timeline to that element's events.

## Chain Orchestrator

The Chain Orchestrator is an AI assistant that helps you plan, build, and execute chains. It operates in three modes:

### Plan Mode

The orchestrator analyzes your task, optionally runs reconnaissance on target nodes, and proposes a plan for what chains and operations to build. The plan is displayed with step-by-step progress tracking.

### Build Mode

The orchestrator collaboratively constructs chains with you in real-time. When the agent uses its `update_workspace` tool, the chain builder in your active tab updates automatically — you can see nodes appear and connections form as the agent builds. The agent knows all nine element types, connection conditions, session groups, block configuration, and targeting.

You can also edit the chain builder directly while the agent works, enabling true pair-building.

### Execute Mode

The orchestrator runs chains and operations on target nodes using its full set of MCP tools. Execution events stream into the event timeline for real-time monitoring. Fan-out execution with TargetSpec allows targeting multiple nodes and agents.

### Starting a Session

1. Ensure an LLM provider is configured in Settings
2. Click **Start** in the orchestrator pane header
3. Select a mode (Plan, Build, or Execute)
4. Type your request in the input bar

The orchestrator has access to all MCP tools available to the main Orchestrator, plus additional tools for workspace manipulation:

| Tool | Description |
|------|-------------|
| `update_workspace` | Push a chain definition to a tab |
| `set_mode` | Switch between plan/build/execute |
| `create_tab` | Create a new tab |
| `create_op_definition` | Create a new operation definition |
| `report_plan` | Display a plan with steps |
| `wait` | Pause execution |

## Workflow Examples

### Building a Chain from Scratch

1. Open the Execution page
2. Start a Chain Orchestrator session in Build mode
3. Describe what you want: "Build a chain that runs credential harvesting, stores results in memory, loops through targets, transforms the output into a report, and outputs it"
4. Watch as the orchestrator constructs the chain in your tab
5. Edit any elements as needed
6. Save and run

### Executing on Multiple Nodes

1. Build your chain in one tab
2. Save it
3. Switch to Execute mode in the orchestrator
4. Ask: "Run this chain on all Windows nodes"
5. The orchestrator selects nodes via TargetSpec and agents, running the chain with fan-out execution

### Planning a Campaign

1. Start in Plan mode
2. Ask: "Plan an operation to enumerate credentials across the network"
3. Review the proposed plan
4. Switch to Build mode to construct the chains
5. Switch to Execute mode to run them
