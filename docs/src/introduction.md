# Introduction

Praxis is an open-source research and experimentation platform for discovering, controlling, and orchestrating computer-use AI agents across endpoints.

As AI coding agents become more prevalent - tools that can read files, execute commands, and interact directly with systems - understanding their security properties becomes critical. Praxis helps enrich our understanding of what's possible when you have legitimate access to systems where these agents run, and what that means for endpoint security.

Built by [Origin](https://originhq.com) for security research and red team operations.

## Why Does This Exist?

AI coding assistants are everywhere now - Claude Code, Codex CLI, Gemini CLI, Microsoft 365 Copilot. These tools can read your files, execute commands, browse the web, and interact with APIs. From a security perspective, they're incredibly interesting.

Praxis started as a question: what can you do if you have access to a system running one of these agents? Not by exploiting vulnerabilities in the agents themselves, but by using the access you already have to see what they're doing and repurpose their capabilities.

This matters for:

- **Red teams** exploring post-compromise scenarios where AI agents are present
- **Security researchers** understanding the attack surface these tools create
- **Blue teams** wanting to know what visibility they have (or don't have) into agent activity

## What Can Praxis Do?

| Feature | Description |
|---------|-------------|
| **Agent Discovery** | Fingerprint and detect computer-use agents on endpoints |
| **Reconnaissance** | Enumerate tools (MCP servers, skills), configurations, and session histories |
| **Config Visibility** | View and edit agent configuration files directly |
| **Traffic Interception** | MITM proxy for agent-to-LLM traffic |
| **Agent Dialog** | Create interactive sessions with agents |
| **Semantic Operations** | Define and chain natural language tasks for multi-step automation |
| **Chain Automation** | Trigger chains automatically on schedules, intercept matches, or new node events |
| **Toolkit** | Library of built-in offensive operations with chain integration |
| **Terminal Access** | PTY terminal on remote nodes |

## The Three Components

Praxis has three main pieces:

```diagram
┌───────────────────────────────────────────────────────────┐
│                                                           │
│                       Your Browser                        │
│                    (Web UI @ :8080)                       │
│                                                           │
└─────────────────────────────┬─────────────────────────────┘
                              │
                              │
┌─────────────────────────────▼─────────────────────────────┐
│                                                           │
│                         Service                           │
│         (Backend + Database + Operation Manager)          │
│                                                           │
└─────────────────────────────┬─────────────────────────────┘
                              │
                              │ RabbitMQ
                              │
        ┌─────────────────────┴─────────────────────┐
        │                                           │
        │                                           │
┌───────▼───────┐                         ┌─────────▼─────────┐
│               │                         │                   │
│     Node      │                         │       Node        │
│  (Target #1)  │                         │    (Target #2)    │
│               │                         │                   │
└───────────────┘                         └───────────────────┘
```

**Node** runs on target systems. It discovers agents, intercepts traffic, handles sessions, and reports back to the service. Nodes are stateless - all the interesting data lives on the service.

**Service** is the central backend. It stores operation definitions, chain workflows, intercepted traffic, and recon results. It also runs the semantic operations manager that orchestrates agent tasks.

**Web** is the React frontend that talks to the service over WebSocket. It provides the UI for everything - selecting nodes, viewing agents, running operations, building chains.

## Early Release Notice

This is an early release to showcase initial capabilities. It is **not yet ready** for full-scale red teaming or production use - although you can certainly experiment to your heart's content.

The platform is under active development:

- Some features are incomplete or experimental
- The codebase is evolving rapidly
- **This is not designed to be stealthy** - it installs root certificates, modifies system settings, and is generally quite noisy

We're releasing early to get feedback and contributions from the community.

## Getting Started

Ready to try it out? Head to the [Installation](./getting-started/installation.md) guide.
