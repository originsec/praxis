# Introduction

Praxis is a semantic command and control framework for AI agents. If that sounds like a mouthful, here's what it actually means: it's a platform for discovering, monitoring, and interacting with computer-use AI agents running on endpoints.

## Why Does This Exist?

AI coding assistants are everywhere now-Claude Code, Gemini CLI, GitHub Copilot, Microsoft 365 Copilot. These tools can read your files, execute commands, browse the web, and interact with APIs. They're incredibly useful, but from a security perspective, they're also incredibly interesting.

Praxis started as a question: what can you do if you have access to a system running one of these agents? Not by exploiting vulnerabilities in the agents themselves, but by using the access you already have to see what they're doing and potentially redirect their capabilities.

This matters for:

- **Red teams** exploring post-compromise scenarios where AI agents are present
- **Security researchers** understanding the attack surface these tools create
- **Blue teams** wanting to know what visibility they have (or don't have) into agent activity

## What Can Praxis Do?

At its core, Praxis lets you:

**Discover agents** - Find out what AI assistants are installed and running on a system. The node component fingerprints common agents and reports what it finds.

**See what they see** - Reconnaissance shows you the agent's tools (MCP servers, skills, plugins), configuration files, and session histories. You can see what projects they've been used on and what conversations have happened.

**Watch the traffic** - Intercept the HTTPS traffic between agents and their LLM backends. See the prompts being sent, the responses coming back, the tool calls being made.

**Talk to them** - Create sessions where you can send prompts directly to agents, either using their existing context or starting fresh.

**Automate with semantic operations** - Define tasks in natural language ("find all files containing API keys and list them") and let the agent figure out how to accomplish them.

**Chain operations together** - Build visual workflows that connect multiple operations, transforming outputs and passing them between steps.

## The Three Components

Praxis has three main pieces:

```
┌─────────────────────────────────────────────────────────────┐
│                        Your Browser                          │
│                     (Web UI @ :8080)                         │
└────────────────────────────┬────────────────────────────────┘
                             │
┌────────────────────────────▼────────────────────────────────┐
│                         Service                              │
│           (Backend + Database + Operation Manager)           │
└────────────────────────────┬────────────────────────────────┘
                             │ RabbitMQ
        ┌────────────────────┴────────────────────┐
        │                                          │
┌───────▼───────┐                         ┌───────▼───────┐
│     Node      │                         │     Node      │
│  (Target #1)  │                         │  (Target #2)  │
└───────────────┘                         └───────────────┘
```

**Node** runs on target systems. It discovers agents, intercepts traffic, handles sessions, and reports back to the service. Nodes are stateless-all the interesting data lives on the service.

**Service** is the central backend. It stores operation definitions, chain workflows, intercepted traffic, and recon results. It also runs the semantic operations manager that orchestrates agent tasks.

**Web** is the React frontend that talks to the service over WebSocket. It provides the UI for everything-selecting nodes, viewing agents, running operations, building chains.

## Early Days

Fair warning: this is an early release. We're putting it out there to get feedback and contributions, but it's not production-ready. Some things are rough around the edges, the documentation is still being written, and the API might change.

Most importantly: **Praxis is not stealthy**. It installs root CA certificates, modifies system proxy settings, adds hosts file entries, and generally leaves traces everywhere. It's a research tool, not a covert implant.

## Getting Started

Ready to try it out? Head to the [Installation](./getting-started/installation.md) guide.
