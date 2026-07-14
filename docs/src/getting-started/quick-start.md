# Quick Start

Let's walk through the basic workflow: connecting a node, discovering an agent, running recon, and executing an operation.

## Prerequisites

You should have:
- Praxis service running (via Docker or native build)
- At least one LLM configured (see [Configuration](./configuration.md))
- A node running on a system with an AI agent installed
- The `praxis` TUI installed (see [Installation](./installation.md))

## Step 1: Check Your Node

Launch the TUI:

```bash
praxis
```

Open the **Nodes** window with `Ctrl+L`. You should see your node in the
node list. Use the arrow keys (or click) to select it. The detail pane
shows:

- **Machine name** and OS details
- **Detected agents** — which AI assistants were found
- **Status** of interception, sessions, etc.

If no agents show up, make sure the target system actually has Claude Code, Codex CLI, Gemini CLI, or another supported agent installed and configured.

## Step 2: Select an Agent

In the Nodes window, focus the agent list and select one. This focuses
all subsequent operations on that specific agent.

## Step 3: Run Reconnaissance

With an agent selected, press `r` to open the **Recon** overlay. This
performs static reconnaissance:

- Discovers **MCP servers** and other tool integrations
- Lists **configuration files** and their contents
- Shows **session history** — past conversations and their locations
- Enumerates **project paths** where the agent has been used

Switch tabs with `Tab` (or `1` `2` `3`) to browse Config, Tools, and
Sessions. Press `r` to refresh static recon.

### Semantic Recon

For deeper discovery, press `d` to run semantic recon (requires an LLM
configured for "Semantic Parser"). This uses the LLM to parse
configuration files and extract tool definitions that might not be
obvious from static analysis. It also creates sessions and communicates
directly with the agent to discover its full capabilities, so it takes
longer than static recon.

## Step 4: Look Around

With recon data, you can:

**View configuration files** — In the Config tab, pick any file to see
its contents.

**Browse sessions** — In the Sessions tab, see what conversations the
agent has had and which projects it's worked on.

**Check tools** — In the Tools tab, see what MCP servers, skills, or
plugins are available to the agent.

## Step 5: Create a Session

In the Nodes window, with an agent selected, start a session chat. You
can specify a working directory and toggle YOLO mode.

**Working Directory** — where the agent should operate. Affects what
files it can see and work with.

**YOLO Mode** — when enabled, the agent auto-approves all tool calls
without asking for confirmation. Use this for automation, but be
careful — it will execute whatever the agent decides to run.

Once the session is created, send prompts directly from the chat view.

## Step 6: Run an Operation

Operations are predefined tasks you can execute through agents. The library starts empty, so let's create a simple one first.

### Create Your First Operation

1. Open the **Operations** window (`Ctrl+P`) and switch to the **Library** tab
2. Create a new operation
3. Fill in:
   - **Name**: `hello-world`
   - **Category**: `test`
   - **Description**: `A simple test operation`
   - **Prompt**: `Say hello and tell me what directory you're currently in.`
   - **Mode**: `one-shot`
   - **Timeout**: `60`
4. Save

### Run It

1. Switch to the **Executions** tab
2. Run the operation, selecting your node and agent
3. Choose `test::hello-world`

The operation executes through your agent. Watch the output in
real-time in the Executions tab — you'll see the agent's response
appear as it completes.

### Operation Modes

- **One-shot** - sends the prompt directly to the agent and returns the response
- **Agent** - uses an orchestrating LLM to run multi-turn interactions with the target agent (useful for complex tasks)

For more complex workflows, you can chain multiple operations together. See [Semantic Operations](../usage/semantic-operations.md) for details.

## Step 7: Enable Interception (Optional)

To see the traffic between the agent and its LLM backend, open the
**Intercept** window (`Ctrl+T`):

1. Select your node
2. Choose a method:
   - **Proxy** - configures system proxy settings
   - **VPN** - uses a TUN adapter for packet-level routing
   - **Hosts** - modifies the hosts file
3. Enable interception

Captured traffic streams into the **Log** tab. You can see:
- Full request/response bodies
- Prompts and completions
- Tool calls and results

See [Interception](../usage/interception.md) for details on each method.

## What's Next?

- [Configure LLM providers](./configuration.md) for semantic features
- [Learn about agent connectors](../connectors/overview.md) and their capabilities
- [Set up traffic interception](../usage/interception.md) in detail
- [Build operation chains](../usage/semantic-operations.md) for automation
