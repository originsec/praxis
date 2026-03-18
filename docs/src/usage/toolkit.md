# Toolkit

The Toolkit provides a library of built-in offensive operations that run directly against target agents. Each tool is a self-contained operation with its own configuration and execution logic, managed through the **Toolkit** page in the web UI.

## Accessing the Toolkit

Go to **Toolkit** in the sidebar. The page lists all available tools with their descriptions and configuration options.

## Available Tools

### Session History Poisoning

Reads session history from a target agent, uses an LLM to transform messages for injection purposes, and writes the modified history back. Requires node/agent selection and an LLM model.

### Message Encoder

Encodes text payloads using various encoding schemes (Base64, Hex, ROT13, Morse, Fullwidth Unicode, Unicode Tags, Braille, Upside Down). Standalone tool — no node interaction required.

### LLMMap

Generates prompt injection payloads using [LLMMap](https://github.com/Hellsender01/LLMMap) transforms. Standalone tool — no node interaction required. Requires a running LLMMap instance with the `--api` flag.

Configuration:
- **Goal** — describe what the injection should achieve
- **Transform** — obfuscation method (None, Spacing, Unicode Injection, Base64, Wrapper Framing)
- **Intensity** — aggressiveness level (1-5)

The LLMMap REST endpoint URL is configured in **Settings** > **Toolkit**.

## Running a Tool

1. Select a tool from the list
2. Configure any required parameters
3. For tools that require targeting, select the target node and agent
4. Click **Run** or **Generate**

Execution results appear inline on the Toolkit page.

## Chain Integration

Toolkit operations can be used as elements in operation chains. When building a chain, toolkit operations are available from the element palette alongside standard operations. This allows you to compose toolkit operations with transforms, memory, and other chain elements into automated workflows.

## Managing Tools

Tools are managed at the service level. The Toolkit page provides full CRUD access — you can view, configure, and execute tools from a single interface.
