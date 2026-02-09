# Quick Start

Let's walk through the basic workflow: connecting a node, discovering an agent, running recon, and executing an operation.

## Prerequisites

You should have:
- Praxis service running (via Docker or native build)
- At least one LLM configured (see [Configuration](./configuration.md))
- A node running on a system with an AI agent installed

## Step 1: Check Your Node

Open the web UI at **http://localhost:8080**. You should see your node in the left sidebar under the node list.

Click on it to select it. The main panel shows:
- **Machine name** and OS details
- **Detected agents** - which AI assistants were found
- **Status** of interception, sessions, etc.

If no agents show up, make sure the target system actually has Claude Code, Codex CLI, Gemini CLI, or another supported agent installed and configured.

## Step 2: Select an Agent

From the agent list, click on one to select it. This focuses all operations on that specific agent.

The agent panel shows:
- **Name** and type
- **Session status** - whether there's an active session
- **Recon data** - if you've run reconnaissance

## Step 3: Run Reconnaissance

Click the **Recon** button (or go to the Recon tab). This performs static reconnaissance:

- Discovers **MCP servers** and other tool integrations
- Lists **configuration files** and their contents
- Shows **session history** - past conversations and their locations
- Enumerates **project paths** where the agent has been used

The results appear in the Recon panel, organized by category.

### Semantic Recon

For deeper discovery, click the **Discover** button to run semantic recon (requires an LLM configured for "Semantic Parser"). This uses the LLM to parse configuration files and extract tool definitions that might not be obvious from static analysis. It also creates sessions and communicates directly with the agent to discover its full capabilities, so it takes longer than static recon.

## Step 4: Look Around

With recon data, you can:

**View configuration files** - Click on any config file to see its contents. Some files can be edited directly (like Claude's `config.json` or MCP server definitions).

**Browse sessions** - See what conversations the agent has had, which projects it's worked on.

**Check tools** - See what MCP servers, skills, or plugins are available to the agent.

## Step 5: Create a Session

Click **Create Session** to start an interactive session with the agent. This spawns the agent process in a controlled context where you can send prompts and receive responses.

**Working Directory** - You can specify where the agent should operate. This affects what files it can see and work with.

**YOLO Mode** - When enabled, the agent auto-approves all tool calls without asking for confirmation. Use this for automation, but be careful-it will execute whatever the agent decides to run.

Once the session is created, you can send prompts directly from the Sessions panel.

## Step 6: Run an Operation

Operations are predefined tasks you can execute through agents. The library starts empty, so let's create a simple one first.

### Create Your First Operation

1. Go to **Operations** → **Library**
2. Click **New Operation**
3. Fill in:
   - **Name**: `hello-world`
   - **Category**: `test`
   - **Description**: `A simple test operation`
   - **Prompt**: `Say hello and tell me what directory you're currently in.`
   - **Mode**: `one-shot`
   - **Timeout**: `60`
4. Click **Save**

### Run It

1. Go to **Operations** → **Runs**
2. Click **Run Operation**
3. Select your node and agent
4. Choose `test::hello-world` from the dropdown
5. Click **Run**

The operation executes through your agent. Watch the output in real-time in the Runs tab - you'll see the agent's response appear as it completes.

### Operation Modes

- **One-shot** - sends the prompt directly to the agent and returns the response
- **Agent** - uses an orchestrating LLM to run multi-turn interactions with the target agent (useful for complex tasks)

For more complex workflows, you can chain multiple operations together with the visual chain builder. See [Semantic Operations](../usage/semantic-operations.md) for details.

## Step 7: Enable Interception (Optional)

To see the traffic between the agent and its LLM backend:

1. Go to **Intercept**
2. Select your node
3. Choose a method:
   - **Proxy** - configures system proxy settings
   - **VPN** - uses a TUN adapter for packet-level routing
   - **Hosts** - modifies the hosts file
4. Click **Enable**

Traffic appears in the **Traffic** tab. You can see:
- Full request/response bodies
- Prompts and completions
- Tool calls and results

See [Interception](../usage/interception.md) for details on each method.

## What's Next?

- [Configure LLM providers](./configuration.md) for semantic features
- [Learn about agent connectors](../connectors/overview.md) and their capabilities
- [Set up traffic interception](../usage/interception.md) in detail
- [Build operation chains](../usage/semantic-operations.md) for automation
