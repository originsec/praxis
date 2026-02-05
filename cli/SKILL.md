# Praxis CLI Skill

## What is Praxis?

Praxis is a Command & Control (C2) framework for orchestrating AI coding agents. It provides a unified interface to manage, monitor, and interact with AI agents (like Claude Code, Cursor, Windsurf, etc.) running on remote machines.

## When to Use This Skill

Use the `praxis_cli` command when the user wants to:
- List or manage nodes (machines running Praxis agents)
- List or select AI agents on nodes
- Create sessions with agents and send prompts
- Run semantic operations (pre-configured AI workflows)
- Run chains (multi-step operation workflows)
- Search intercepted network traffic
- Interact with the Praxis C2 network programmatically

## How to Use

Before using any commands, first discover the full capabilities by running:

```bash
praxis_cli --fullhelp
```

This outputs comprehensive documentation for all commands and subcommands, including:
- Global options (RabbitMQ URL, output format, timeout)
- Node management commands
- Agent management commands
- Session commands for interacting with agents
- Semantic operation commands
- Chain workflow commands
- Traffic search commands

## Key Concepts

- **Node**: A machine running the Praxis node agent
- **Agent**: An AI coding agent (e.g., Claude Code) discovered on a node
- **Session**: An active connection to an agent for sending prompts
- **Operation**: A pre-configured prompt/workflow for common tasks
- **Chain**: A sequence of operations executed as a workflow

## Requirements

The Praxis service must be running and accessible via RabbitMQ. The default connection is `amqp://praxis:praxis@localhost:5672`.

To specify a different RabbitMQ URL:
```bash
praxis_cli --rabbitmq amqp://user:pass@host:5672 node list
```

Or set the environment variable:
```bash
export PRAXIS_RABBITMQ_URL=amqp://user:pass@host:5672
```

## Output Formats

Use `--output json` for machine-readable output suitable for scripting and parsing.
