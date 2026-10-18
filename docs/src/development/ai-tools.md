# Native AI tool calls

`common::ai` represents provider tool calls independently of their wire format.
The orchestrator can send JSON Schema tool definitions directly to a provider
instead of asking the model to print tool calls inside ordinary text.
Text-only callers and previously serialized messages remain compatible.

## Conversation contract

Use `ChatCompletionRequest::with_tools(Vec<Tool>)` to advertise tools. Assistant
messages retain a `Vec<ToolCall>` with the provider call ID, name, and JSON object
arguments. Append the complete assistant message, followed by one
`Message::tool_result(&call, output)` per call, before requesting the next turn.
Keep all calls from one assistant turn together and return all their results.

```rust,ignore
let request = ChatCompletionRequest::new(model, history.clone())
    .with_tools(tools);
let response = client.chat_completion(request).await?;
let assistant = response.choices[0].message.clone();
let calls = assistant.tool_calls.clone();
history.push(assistant);
for call in calls {
    let output = dispatch_validated_tool(&call.name, &call.arguments).await?;
    history.push(Message::tool_result(&call, output));
}
```

The transport validates call IDs, names, and JSON object arguments. The caller
must validate tool membership, its argument schema, scope, and execution policy
before dispatch. Receiving a call does not authorize its execution.

## Streaming and cancellation

All three transports support `chat_completion_stream`. Text arrives incrementally;
`ChatCompletionDelta::tool_calls` contains complete calls only once a successful
terminal event has arrived. OpenAI argument fragments are assembled by call index;
Anthropic input fragments are assembled by content-block index. Gemini's default
Generate Content stream provides complete function-call parts. Praxis does not
request Gemini's optional partial-argument streaming mode and rejects unexpected
partial arguments.

Consume the whole stream successfully before executing its calls. Invalid JSON,
incomplete tool arguments, interrupted tool responses, provider error events,
and streams that close before completion return errors. A usage-only event can
arrive after the event carrying calls. Dropping the stream or its in-flight
completion future cancels the HTTP operation; there is no detached request task
or mutex held across the network wait.

When collecting streamed history, retain both `tool_calls` and `provider_data`
from the deltas on the assistant message. `provider_data` contains opaque
Anthropic content blocks or Gemini parts, including thought signatures. Persist
these unchanged with the message: rebuilding a signed assistant turn from text
and tool arguments alone can cause the provider to reject subsequent requests.
Do not mix provider-specific histories across models without starting a new
conversation.

## Provider compatibility

- **OpenAI-compatible:** uses `/chat/completions`, `tools`, assistant `tool_calls`,
  and `tool` result messages. Existing custom endpoints retain their URL behavior.
  The endpoint and selected model must support Chat Completions function calling.
  The Responses API is not implemented here; models requiring it for tools,
  including GPT-6 Astra and GPT-6.1 Sol, need a separate transport or a compatible
  model selection. Praxis does not silently replace the selected model.
- **Anthropic:** uses Messages API tool definitions, `tool_use` blocks, and
  `tool_result` blocks. Consecutive parallel results share one user turn, and all
  system messages are retained in the system instruction.
- **Gemini:** uses Generate Content function declarations and `functionResponse`
  parts, with the API key in a header. Provider-issued function IDs are preserved.
  Older responses without IDs get a local correlation ID that is not sent back
  to Gemini. Original signed response parts are retained.

The legacy text parser remains available for existing consumers and explicit
compatibility modes; native calls do not pass through that parser.

## Verification

Run `cargo test -p praxis_common --lib ai::`. Fixtures cover provider request and
response shapes, interleaved calls, signed-history round trips, parallel results,
malformed/truncated responses, SSE framing, usage-only events, and cancellation
while waiting for stream data. These tests run locally without provider API calls.

Protocol references: [OpenAI function calling](https://developers.openai.com/api/docs/guides/function-calling),
[Anthropic streaming](https://platform.claude.com/docs/en/build-with-claude/streaming),
[Gemini function calling](https://ai.google.dev/gemini-api/docs/generate-content/function-calling),
and [Gemini thought signatures](https://ai.google.dev/gemini-api/docs/generate-content/thought-signatures).
