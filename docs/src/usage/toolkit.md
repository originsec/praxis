# Toolkit

The Toolkit provides a library of built-in offensive operations that run directly against target agents. Each tool is a self-contained operation with its own configuration and execution logic, registered in the service.

## Invoking Tools

Toolkit tools are surfaced through:

- **Chains** — Tool elements in chain definitions invoke registered toolkit tools as part of a workflow.
- **MCP Server** — toolkit tools are exposed as MCP tools for external AI agents and the built-in Orchestrator.

## Action Log

Toolkit executions are recorded in the `ToolkitActionsLog` table and can be queried from the TUI's **Log Query** window (`Ctrl+G`). See [Log Query](./log-query.md).

## Chain Integration

Toolkit operations can be used as elements in operation chains. This allows you to compose toolkit operations with transforms, memory, and other chain elements into automated workflows.
