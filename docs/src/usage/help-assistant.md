# Help Assistant

The Help Assistant is a documentation-aware chat agent built into the terminal
UI. It is seeded with the Praxis documentation and answers natural-language
questions about how to use Praxis — features, configuration, workflows, and
concepts — without leaving the app.

It is deliberately separate from the [Orchestrator](./orchestrator.md): the
Orchestrator is an operator that plans and executes campaigns by driving nodes
and agents, whereas the Help Assistant is a read-only guide. It can only search
the documentation bundled with Praxis and never takes actions on your behalf.

## Opening the assistant

Press `Ctrl+H` from **any** window to summon the Help Assistant overlay. Press
`Ctrl+H` again, or `Esc`, to dismiss it. Because the overlay floats above the
current window, you can ask a question and return to exactly what you were
doing.

| Key | Action |
| --- | --- |
| `Ctrl+H` | Open / close the assistant |
| `Enter` | Send the current question |
| `Ctrl+T` | Toggle inclusion of screen context (when available) |
| `Ctrl+C` | Stop a streaming response (keeps the overlay open) |
| `Ctrl+L` | Clear the conversation |
| `Esc` | Close the assistant (cancels any in-flight response) |
| `Up` / `Down` / `PageUp` / `PageDown` | Scroll the conversation |
| Mouse wheel | Scroll the conversation |

Every question is sent to the model assigned to the Documentation Helper. The
model decides whether the available conversation and screen context are enough
to answer directly or whether it should search and read the bundled
documentation. The overlay shows a neutral thinking indicator while the model
is deciding; when it calls a documentation tool, it instead shows a
documentation-specific divider and spinner before the final answer. Tool-turn
narration is limited to the first acknowledgement, so a multi-step lookup has
a helpful initial response without several repeated partial replies. The terminal renders
the common Markdown used in answers: headings, bullet lists, bold text, inline
code, block quotes, and fenced code blocks. Closing the overlay while a
response is streaming cancels it, so nothing keeps running in the background.

## Screen context

When you open the assistant, it captures a short, structured description of the
window you were looking at (for example, "the Nodes window, 3 nodes connected")
and includes it with your question so answers can be specific to what is on
screen. The footer shows the current context source and whether it is included;
`Ctrl+T` toggles it off for the next question.

Only low-sensitivity, structural context is ever included — which window you are
on and safe counts. Sensitive data such as session output, intercepted request
and response bodies, captured credentials, and log rows is **never** collected
or sent to the model provider.

## Configuration

The assistant uses the model assigned to the **Documentation Helper** feature
under **Settings > LLM Providers > Feature Selection**. If no model is assigned,
it falls back to the model configured for the Orchestrator, so it works out of
the box once any conversational model is configured.

Relevant service configuration keys:

| Key | Description |
| --- | --- |
| `llm_feature_doc_helper` | Model definition assigned to the Help Assistant. Falls back to `llm_feature_orchestrator` when unset. |

The documentation corpus is embedded into the service at build time, so the
assistant's answers reflect the documentation shipped with your Praxis version
and require no network access to the docs site.
