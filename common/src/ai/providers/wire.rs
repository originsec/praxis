use std::{collections::BTreeMap, pin::Pin};

use anyhow::{Context, Result, anyhow, bail};
use futures_core::Stream;
use futures_util::StreamExt;
use serde_json::{Value, json};

use crate::ai::{
    ChatCompletionChoice, ChatCompletionDelta, ChatCompletionRequest, ChatCompletionResponse,
    Message, Role, ToolCall, Usage,
};

#[derive(Clone, Copy, Debug)]
pub(super) enum Format {
    OpenAI,
    Anthropic,
    Gemini,
}

fn schema(tool: &crate::ai::Tool) -> Value {
    tool.parameters
        .clone()
        .unwrap_or_else(|| json!({"type":"object","properties":{}}))
}

pub(super) fn request_body(request: &ChatCompletionRequest, format: Format, stream: bool) -> Value {
    let mut messages: Vec<Value> = Vec::new();
    let mut system = Vec::new();
    for message in &request.messages {
        if message.role == Role::System && !matches!(format, Format::OpenAI) {
            system.push(message.text());
            continue;
        }
        let native = match format {
            Format::OpenAI => {
                let mut item = json!({"role": message.role.as_str(), "content": message.text()});
                if !message.tool_calls.is_empty() {
                    item["tool_calls"] = Value::Array(
                        message
                            .tool_calls
                            .iter()
                            .map(|call| {
                                json!({
                                    "id":call.id,"type":"function", "function":{
                                        "name":call.name,"arguments":call.arguments.to_string()
                                    }
                                })
                            })
                            .collect(),
                    );
                }
                if let Some(id) = &message.tool_call_id {
                    item["tool_call_id"] = json!(id);
                }
                item
            }
            Format::Anthropic => {
                let content =
                    if message.role == Role::Tool {
                        vec![
                            json!({"type":"tool_result", "tool_use_id":message.tool_call_id,
                        "content":message.text()}),
                        ]
                    } else if let Some(blocks) = message
                        .provider_data
                        .as_ref()
                        .and_then(|data| data.get("anthropic"))
                        .and_then(Value::as_array)
                    {
                        blocks.clone()
                    } else {
                        let mut blocks = Vec::new();
                        if !message.text().is_empty() {
                            blocks.push(json!({"type":"text", "text":message.text()}));
                        }
                        blocks.extend(message.tool_calls.iter().map(|call| json!({
                        "type":"tool_use","id":call.id,"name":call.name,"input":call.arguments
                    })));
                        blocks
                    };
                json!({"role":if message.role == Role::Assistant {"assistant"} else {"user"},"content":content})
            }
            Format::Gemini => {
                let parts = if message.role == Role::Tool {
                    let mut response =
                        json!({"name":message.tool_name, "response":{"result":message.text()}});
                    if let Some(id) = &message.tool_call_id {
                        if !id.starts_with("praxis-gemini-") {
                            response["id"] = json!(id);
                        }
                    }
                    vec![json!({"functionResponse":response})]
                } else if let Some(parts) = message
                    .provider_data
                    .as_ref()
                    .and_then(|data| data.get("gemini"))
                    .and_then(Value::as_array)
                {
                    parts.clone()
                } else {
                    let mut parts = Vec::new();
                    if !message.text().is_empty() {
                        parts.push(json!({"text":message.text()}));
                    }
                    parts.extend(message.tool_calls.iter().map(|call| {
                        json!({"functionCall":{
                            "id":call.id,"name":call.name,"args":call.arguments
                        }})
                    }));
                    parts
                };
                json!({"role":if message.role == Role::Assistant {"model"} else {"user"},"parts":parts})
            }
        };
        //
        // Parallel tool results form one user turn in block-based protocols.
        // Keeping them together also preserves Anthropic's immediate-result rule.
        //
        let blocks_key = match format {
            Format::Anthropic => "content",
            _ => "parts",
        };
        if !matches!(format, Format::OpenAI)
            && message.role == Role::Tool
            && messages.last().is_some_and(|last| last["role"] == "user")
        {
            let last = messages.last_mut().unwrap();
            last[blocks_key]
                .as_array_mut()
                .unwrap()
                .extend(native[blocks_key].as_array().unwrap().iter().cloned());
        } else {
            messages.push(native);
        }
    }
    let mut body = match format {
        Format::OpenAI => json!({"model":request.model,"messages":messages}),
        Format::Anthropic => json!({"model":request.model,"messages":messages,
            "max_tokens":request.max_tokens.unwrap_or(4096)}),
        Format::Gemini => json!({"contents":messages,"generationConfig":{}}),
    };
    if !system.is_empty() {
        match format {
            Format::Anthropic => body["system"] = json!(system.join("\n\n")),
            Format::Gemini => {
                body["systemInstruction"] = json!({"parts":[{"text":system.join("\n\n")}]})
            }
            _ => {}
        }
    }
    if matches!(format, Format::Gemini) {
        if let Some(n) = request.max_tokens {
            body["generationConfig"]["maxOutputTokens"] = json!(n);
        }
        if let Some(n) = request.temperature {
            body["generationConfig"]["temperature"] = json!(n);
        }
        if let Some(n) = request.top_p {
            body["generationConfig"]["topP"] = json!(n);
        }
    } else {
        if let Some(n) = request.max_tokens {
            body["max_tokens"] = json!(n);
        }
        if let Some(n) = request.temperature {
            body["temperature"] = json!(n);
        }
        if let Some(n) = request.top_p {
            body["top_p"] = json!(n);
        }
        if stream {
            body["stream"] = json!(true);
        }
    }
    if !request.tools.is_empty() {
        body["tools"] = match format {
            Format::OpenAI => json!(request.tools.iter().map(|tool| json!({"type":"function","function":{
                "name":tool.name,"description":tool.description.as_deref().unwrap_or(""),"parameters":schema(tool)
            }})).collect::<Vec<_>>()),
            Format::Anthropic => json!(request.tools.iter().map(|tool| json!({
                "name":tool.name,"description":tool.description.as_deref().unwrap_or(""),"input_schema":schema(tool)
            })).collect::<Vec<_>>()),
            Format::Gemini => json!([{"functionDeclarations":request.tools.iter().map(|tool| json!({
                "name":tool.name,"description":tool.description.as_deref().unwrap_or(""),"parametersJsonSchema":schema(tool)
            })).collect::<Vec<_>>()}]),
        };
    }
    body
}

fn call(id: &Value, name: &Value, arguments: Value) -> Result<ToolCall> {
    let id = id
        .as_str()
        .filter(|value| !value.is_empty())
        .context("Tool call has no ID")?;
    let name = name
        .as_str()
        .filter(|value| !value.is_empty())
        .context("Tool call has no name")?;
    if !arguments.is_object() {
        bail!("Tool call {name} arguments must be a JSON object");
    }
    Ok(ToolCall {
        id: id.into(),
        name: name.into(),
        arguments,
    })
}

fn assistant(blocks: &[Value], format: Format) -> Result<Message> {
    let mut message = Message::assistant("");
    let mut text = String::new();
    for block in blocks {
        match format {
            Format::Anthropic => {
                if block["type"] == "text" {
                    text.push_str(block["text"].as_str().unwrap_or_default());
                }
                if block["type"] == "tool_use" {
                    message.tool_calls.push(call(
                        &block["id"],
                        &block["name"],
                        block["input"].clone(),
                    )?);
                }
            }
            Format::Gemini => {
                if block["thought"] != true {
                    text.push_str(block["text"].as_str().unwrap_or_default());
                }
                if let Some(function) = block.get("functionCall") {
                    if function.get("partialArgs").is_some() || function["willContinue"] == true {
                        bail!("Unexpected partial Gemini function arguments");
                    }
                    let id = function.get("id").cloned().unwrap_or_else(|| {
                        json!(format!("praxis-gemini-{}", uuid::Uuid::new_v4()))
                    });
                    message.tool_calls.push(call(
                        &id,
                        &function["name"],
                        function.get("args").cloned().unwrap_or(json!({})),
                    )?);
                }
            }
            _ => unreachable!(),
        }
    }
    message.content = text.into();
    message.provider_data = Some(match format {
        Format::Anthropic => json!({"anthropic":blocks}),
        _ => json!({"gemini":blocks}),
    });
    Ok(message)
}

fn usage(value: &Value, format: Format) -> Option<Usage> {
    if !value.is_object() {
        return None;
    }
    let (input, output, total) = match format {
        Format::OpenAI => ("prompt_tokens", "completion_tokens", "total_tokens"),
        Format::Anthropic => ("input_tokens", "output_tokens", ""),
        Format::Gemini => (
            "promptTokenCount",
            "candidatesTokenCount",
            "totalTokenCount",
        ),
    };
    let prompt_tokens = value[input].as_u64().unwrap_or(0) as u32;
    let completion_tokens = value[output].as_u64().unwrap_or(0) as u32;
    Some(Usage {
        prompt_tokens,
        completion_tokens,
        total_tokens: value[total]
            .as_u64()
            .map(|v| v as u32)
            .unwrap_or(prompt_tokens.saturating_add(completion_tokens)),
    })
}

pub(super) fn parse_response(
    value: Value,
    model: &str,
    format: Format,
) -> Result<ChatCompletionResponse> {
    if let Some(error) = value.get("error") {
        bail!("Provider error: {error}");
    }
    let mut choices = Vec::new();
    match format {
        Format::OpenAI => {
            for choice in value["choices"]
                .as_array()
                .context("Response missing choices")?
            {
                let source = &choice["message"];
                let mut message =
                    Message::assistant(source["content"].as_str().unwrap_or_default());
                for item in source["tool_calls"].as_array().into_iter().flatten() {
                    let args = serde_json::from_str(
                        item["function"]["arguments"]
                            .as_str()
                            .context("Missing tool arguments")?,
                    )?;
                    message
                        .tool_calls
                        .push(call(&item["id"], &item["function"]["name"], args)?);
                }
                choices.push(ChatCompletionChoice {
                    index: choice["index"].as_u64().unwrap_or(0) as usize,
                    message,
                    finish_reason: choice["finish_reason"].as_str().map(str::to_owned),
                });
            }
        }
        Format::Anthropic => {
            choices.push(ChatCompletionChoice {
                index: 0,
                message: assistant(
                    value["content"]
                        .as_array()
                        .context("Response missing content")?,
                    format,
                )?,
                finish_reason: value["stop_reason"].as_str().map(str::to_owned),
            });
        }
        Format::Gemini => {
            for (index, candidate) in value["candidates"]
                .as_array()
                .context("Gemini returned no candidates")?
                .iter()
                .enumerate()
            {
                choices.push(ChatCompletionChoice {
                    index,
                    message: assistant(
                        candidate["content"]["parts"]
                            .as_array()
                            .map(Vec::as_slice)
                            .unwrap_or(&[]),
                        format,
                    )?,
                    finish_reason: candidate["finishReason"].as_str().map(str::to_owned),
                });
            }
        }
    }
    if choices.is_empty() {
        bail!("Provider returned no choices");
    }
    for choice in &choices {
        if !choice.message.tool_calls.is_empty() {
            let valid = match format {
                Format::OpenAI => {
                    matches!(choice.finish_reason.as_deref(), Some("tool_calls" | "stop"))
                }
                Format::Anthropic => choice.finish_reason.as_deref() == Some("tool_use"),
                Format::Gemini => choice.finish_reason.as_deref() == Some("STOP"),
            };
            if !valid {
                bail!("Tool response interrupted: {:?}", choice.finish_reason);
            }
        }
    }
    Ok(ChatCompletionResponse {
        id: value["id"]
            .as_str()
            .or_else(|| value["responseId"].as_str())
            .unwrap_or_default()
            .into(),
        model: value["model"].as_str().unwrap_or(model).into(),
        choices,
        usage: usage(
            &value[if matches!(format, Format::Gemini) {
                "usageMetadata"
            } else {
                "usage"
            }],
            format,
        ),
    })
}

#[derive(Default)]
struct PartialCall {
    id: String,
    name: String,
    arguments: String,
}

pub(super) struct StreamState {
    format: Format,
    calls: BTreeMap<usize, PartialCall>,
    blocks: BTreeMap<usize, Value>,
    inputs: BTreeMap<usize, String>,
    parts: Vec<Value>,
    usage: Option<Usage>,
    reason: Option<String>,
    complete: bool,
}

impl StreamState {
    pub(super) fn new(format: Format) -> Self {
        Self {
            format,
            calls: BTreeMap::new(),
            blocks: BTreeMap::new(),
            inputs: BTreeMap::new(),
            parts: Vec::new(),
            usage: None,
            reason: None,
            complete: false,
        }
    }

    pub(super) fn push(&mut self, value: Value) -> Result<ChatCompletionDelta> {
        if let Some(error) = value.get("error") {
            bail!("Provider stream error: {error}");
        }
        let mut delta = ChatCompletionDelta::default();
        match self.format {
            Format::OpenAI => {
                delta.usage = usage(&value["usage"], self.format);
                if let Some(choice) = value["choices"].as_array().and_then(|items| items.first()) {
                    delta.content = choice["delta"]["content"]
                        .as_str()
                        .unwrap_or_default()
                        .into();
                    for item in choice["delta"]["tool_calls"]
                        .as_array()
                        .into_iter()
                        .flatten()
                    {
                        let index =
                            item["index"].as_u64().context("Tool delta missing index")? as usize;
                        let pending = self.calls.entry(index).or_default();
                        if let Some(id) = item["id"].as_str() {
                            pending.id.push_str(id);
                        }
                        if let Some(name) = item["function"]["name"].as_str() {
                            pending.name.push_str(name);
                        }
                        if let Some(args) = item["function"]["arguments"].as_str() {
                            pending.arguments.push_str(args);
                        }
                    }
                    if let Some(reason) = choice["finish_reason"].as_str() {
                        self.complete = true;
                        delta.finish_reason = Some(reason.into());
                        if !self.calls.is_empty() && reason != "tool_calls" && reason != "stop" {
                            bail!("Tool call response interrupted: {reason}");
                        }
                        for pending in std::mem::take(&mut self.calls).into_values() {
                            delta.tool_calls.push(call(
                                &json!(pending.id),
                                &json!(pending.name),
                                serde_json::from_str(&pending.arguments)
                                    .context("Invalid streamed tool arguments")?,
                            )?);
                        }
                    }
                }
            }
            Format::Anthropic => {
                let index = value["index"].as_u64().unwrap_or(0) as usize;
                match value["type"].as_str().unwrap_or_default() {
                    "message_start" => self.usage = usage(&value["message"]["usage"], self.format),
                    "content_block_start" => {
                        let block = &value["content_block"];
                        if block["type"] == "text" {
                            delta.content = block["text"].as_str().unwrap_or_default().into();
                        }
                        self.blocks.insert(index, block.clone());
                    }
                    "content_block_delta" => {
                        let source = &value["delta"];
                        match source["type"].as_str().unwrap_or_default() {
                            "input_json_delta" => self
                                .inputs
                                .entry(index)
                                .or_default()
                                .push_str(source["partial_json"].as_str().unwrap_or_default()),
                            "text_delta" | "thinking_delta" | "signature_delta" => {
                                let field = match source["type"].as_str().unwrap() {
                                    "text_delta" => "text",
                                    "thinking_delta" => "thinking",
                                    _ => "signature",
                                };
                                let text = source[field].as_str().unwrap_or_default();
                                let block = self
                                    .blocks
                                    .get_mut(&index)
                                    .context("Delta for unknown content block")?;
                                let combined =
                                    format!("{}{text}", block[field].as_str().unwrap_or_default());
                                block[field] = json!(combined);
                                if field == "text" {
                                    delta.content = text.into();
                                }
                            }
                            _ => {}
                        }
                    }
                    "content_block_stop" => {
                        if let Some(input) = self.inputs.remove(&index) {
                            self.blocks
                                .get_mut(&index)
                                .context("Input for unknown block")?["input"] =
                                serde_json::from_str(&input)
                                    .context("Invalid streamed tool arguments")?;
                        }
                    }
                    "message_delta" => {
                        self.reason = value["delta"]["stop_reason"].as_str().map(str::to_owned);
                        let previous = self.usage.get_or_insert_with(Usage::default);
                        if let Some(output) = value["usage"]["output_tokens"].as_u64() {
                            previous.completion_tokens = output as u32;
                        }
                        previous.total_tokens = previous
                            .prompt_tokens
                            .saturating_add(previous.completion_tokens);
                    }
                    "message_stop" => {
                        if !self.inputs.is_empty() {
                            bail!("Anthropic stream ended with unfinished tool arguments");
                        }
                        let message = assistant(
                            &self.blocks.values().cloned().collect::<Vec<_>>(),
                            self.format,
                        )?;
                        if !message.tool_calls.is_empty()
                            && self.reason.as_deref() != Some("tool_use")
                        {
                            bail!("Anthropic tool response interrupted: {:?}", self.reason);
                        }
                        delta.tool_calls = message.tool_calls;
                        delta.provider_data = message.provider_data;
                        delta.finish_reason = self.reason.clone();
                        delta.usage = self.usage.clone();
                        self.complete = true;
                    }
                    _ => {}
                }
            }
            Format::Gemini => {
                delta.usage = usage(&value["usageMetadata"], self.format);
                if let Some(candidate) = value["candidates"]
                    .as_array()
                    .and_then(|items| items.first())
                {
                    for part in candidate["content"]["parts"]
                        .as_array()
                        .into_iter()
                        .flatten()
                    {
                        if part["thought"] != true {
                            delta
                                .content
                                .push_str(part["text"].as_str().unwrap_or_default());
                        }
                        self.parts.push(part.clone());
                    }
                    if let Some(reason) = candidate["finishReason"].as_str() {
                        let message = assistant(&self.parts, self.format)?;
                        if !message.tool_calls.is_empty() && reason != "STOP" {
                            bail!("Gemini tool response interrupted: {reason}");
                        }
                        delta.tool_calls = message.tool_calls;
                        delta.provider_data = message.provider_data;
                        delta.finish_reason = Some(reason.into());
                        self.complete = true;
                    }
                }
            }
        }
        Ok(delta)
    }
}

pub(super) async fn checked(response: reqwest::Response) -> Result<reqwest::Response> {
    if !response.status().is_success() {
        let status = response.status();
        bail!(
            "AI provider HTTP error {status}: {}",
            response.text().await.unwrap_or_default()
        );
    }
    Ok(response)
}

pub(super) fn stream(
    response: reqwest::Response,
    format: Format,
) -> Pin<Box<dyn Stream<Item = Result<ChatCompletionDelta>> + Send>> {
    let bytes = response
        .bytes_stream()
        .map(|result| result.map_err(std::io::Error::other));
    decode_sse(tokio_util::io::StreamReader::new(bytes), format)
}

fn decode_sse<R: tokio::io::AsyncBufRead + Unpin + Send + 'static>(
    reader: R,
    format: Format,
) -> Pin<Box<dyn Stream<Item = Result<ChatCompletionDelta>> + Send>> {
    Box::pin(async_stream::try_stream! {
        use tokio::io::AsyncBufReadExt;
        let mut lines = reader.lines();
        let mut state = StreamState::new(format);
        let mut data = String::new();
        loop {
            let line = lines.next_line().await.context("AI stream read failed")?;
            let eof = line.is_none();
            let line = line.unwrap_or_default();
            if line.is_empty() {
                if !data.is_empty() {
                    let payload = std::mem::take(&mut data);
                    if payload.trim() == "[DONE]" { break; }
                    let value = serde_json::from_str(&payload).context("Invalid AI stream JSON")?;
                    yield state.push(value)?;
                }
                if eof { break; }
            } else if let Some(payload) = line.strip_prefix("data:") {
                if !data.is_empty() { data.push('\n'); }
                data.push_str(payload.strip_prefix(' ').unwrap_or(payload));
            }
        }
        if !state.complete { Err(anyhow!("AI stream closed before response completion"))?; }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::Tool;

    fn example_call() -> ToolCall {
        ToolCall {
            id: "call_1".into(),
            name: "lookup".into(),
            arguments: json!({"key":"value"}),
        }
    }

    fn request() -> ChatCompletionRequest {
        let call = example_call();
        ChatCompletionRequest::new(
            "test-model",
            vec![
                Message::system("First instruction"),
                Message::system("Pinned objective"),
                Message::user("Investigate"),
                Message::assistant_with_tools("Checking", vec![call.clone()]),
                Message::tool_result(&call, "Found it"),
            ],
        )
        .with_tools(vec![Tool::new("lookup").with_parameters(json!({
            "type":"object", "properties":{"key":{"type":"string"}},"required":["key"]
        }))])
    }

    #[test]
    fn openai_wire_preserves_native_call_and_result() {
        let body = request_body(&request(), Format::OpenAI, true);
        assert_eq!(body["tools"][0]["function"]["name"], "lookup");
        assert_eq!(body["messages"][3]["tool_calls"][0]["id"], "call_1");
        assert_eq!(body["messages"][4]["role"], "tool");
        assert_eq!(body["messages"][4]["tool_call_id"], "call_1");
        assert_eq!(body["stream"], true);
        let response = parse_response(json!({"id":"r1","model":"m", "choices":[{
            "index":0,"message":{"role":"assistant","content":null,"tool_calls":[{
                "id":"call_1","type":"function","function":{"name":"lookup","arguments":"{\"key\":\"value\"}"}
            }]},"finish_reason":"tool_calls"
        }]}), "m", Format::OpenAI).unwrap();
        assert_eq!(response.choices[0].message.tool_calls, vec![example_call()]);
    }

    #[test]
    fn openai_stream_assembles_interleaved_calls_and_usage() {
        let mut state = StreamState::new(Format::OpenAI);
        let first = state
            .push(json!({"choices":[{"delta":{"tool_calls":[
                {"index":1,"id":"b","function":{"name":"second","arguments":"{\"count\":"}},
                {"index":0,"id":"a","function":{"name":"first","arguments":"{"}}
            ]}}]}))
            .unwrap();
        assert!(first.tool_calls.is_empty());
        state
            .push(json!({"choices":[{"delta":{"tool_calls":[
                {"index":0,"function":{"arguments":"}"}},
                {"index":1,"function":{"arguments":"2}"}}
            ]}}]}))
            .unwrap();
        let final_chunk = state
            .push(json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}))
            .unwrap();
        assert_eq!(final_chunk.tool_calls[0].id, "a");
        assert_eq!(final_chunk.tool_calls[1].arguments, json!({"count":2}));
        let final_usage = state.push(json!({"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}})).unwrap();
        assert_eq!(final_usage.usage.unwrap().total_tokens, 15);
    }

    #[test]
    fn malformed_or_truncated_calls_never_become_executable() {
        for (args, reason) in [("{", "tool_calls"), ("{}", "length"), ("[]", "tool_calls")] {
            let mut state = StreamState::new(Format::OpenAI);
            state
                .push(json!({"choices":[{"delta":{"tool_calls":[{
                    "index":0,"id":"call_1","function":{"name":"lookup","arguments":args}
                }]}}]}))
                .unwrap();
            assert!(
                state
                    .push(json!({"choices":[{"delta":{},"finish_reason":reason}]}))
                    .is_err()
            );
        }
        let mut state = StreamState::new(Format::OpenAI);
        assert!(
            state
                .push(json!({"error":{"message":"quota exhausted"}}))
                .is_err()
        );
    }

    #[test]
    fn anthropic_stream_preserves_signed_thinking_and_tool_inputs() {
        let mut state = StreamState::new(Format::Anthropic);
        for event in [
            json!({"type":"message_start","message":{"usage":{"input_tokens":12,"output_tokens":1}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Inspect state"}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"opaque"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"call_1","name":"lookup","input":{}}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"key\":"}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"\"value\"}"}}),
            json!({"type":"content_block_stop","index":1}),
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":8}}),
        ] {
            assert!(state.push(event).unwrap().tool_calls.is_empty());
        }
        let final_chunk = state.push(json!({"type":"message_stop"})).unwrap();
        assert_eq!(final_chunk.tool_calls, vec![example_call()]);
        assert_eq!(final_chunk.usage.unwrap().total_tokens, 20);
        let mut message = Message::assistant_with_tools("", final_chunk.tool_calls);
        message.provider_data = final_chunk.provider_data;
        let mut req = request();
        req.messages[3] = message;
        let body = request_body(&req, Format::Anthropic, true);
        assert_eq!(body["system"], "First instruction\n\nPinned objective");
        assert_eq!(body["messages"][1]["content"][0]["signature"], "opaque");
        assert_eq!(body["messages"][2]["content"][0]["tool_use_id"], "call_1");
    }

    #[test]
    fn parallel_anthropic_results_share_a_user_turn() {
        let mut req = request();
        let other = ToolCall {
            id: "call_2".into(),
            ..example_call()
        };
        req.messages[3].tool_calls.push(other.clone());
        req.messages
            .push(Message::tool_result(&other, "Second result"));
        let body = request_body(&req, Format::Anthropic, false);
        assert_eq!(body["messages"].as_array().unwrap().len(), 3);
        assert_eq!(body["messages"][2]["content"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn gemini_preserves_signed_parts_and_associates_result_id() {
        let mut state = StreamState::new(Format::Gemini);
        let parts = json!([
            {"text":"Checking"},
            {"functionCall":{"id":"call_1","name":"lookup","args":{"key":"value"}},"thoughtSignature":"opaque-signature"}
        ]);
        let delta = state
            .push(
                json!({"candidates":[{"content":{"parts":parts},"finishReason":"STOP"}],
            "usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":4,"totalTokenCount":14}}),
            )
            .unwrap();
        assert_eq!(delta.tool_calls, vec![example_call()]);
        let mut req = request();
        req.messages[3].provider_data = delta.provider_data;
        let body = request_body(&req, Format::Gemini, true);
        assert_eq!(body["contents"][1]["parts"], parts);
        assert_eq!(
            body["contents"][2]["parts"][0]["functionResponse"]["id"],
            "call_1"
        );
        assert_eq!(
            body["tools"][0]["functionDeclarations"][0]["parametersJsonSchema"]["required"],
            json!(["key"])
        );
        assert_eq!(
            body["systemInstruction"]["parts"][0]["text"],
            "First instruction\n\nPinned objective"
        );
    }

    #[tokio::test]
    async fn sse_supports_crlf_multiline_data_and_final_event_without_blank_line() {
        let fixture: &[u8] = b": heartbeat\r\nevent: delta\r\ndata: {\r\ndata: \"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\r\n\r\ndata:{\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}";
        let chunks = decode_sse(fixture, Format::OpenAI)
            .collect::<Vec<_>>()
            .await;
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].as_ref().unwrap().content, "hello");
        assert_eq!(
            chunks[1].as_ref().unwrap().finish_reason.as_deref(),
            Some("stop")
        );
    }

    #[tokio::test]
    async fn sse_rejects_premature_done_and_malformed_payloads() {
        for fixture in [
            b"data: [DONE]\n\n".as_slice(),
            b"data: not-json\n\n".as_slice(),
        ] {
            let chunks = decode_sse(fixture, Format::OpenAI)
                .collect::<Vec<_>>()
                .await;
            assert!(chunks.last().unwrap().is_err());
        }
    }

    #[tokio::test]
    async fn dropping_pending_stream_releases_reader_immediately() {
        use tokio::io::AsyncWriteExt;
        let (reader, mut writer) = tokio::io::duplex(1024);
        let mut stream = decode_sse(tokio::io::BufReader::new(reader), Format::OpenAI);
        assert!(futures_util::poll!(stream.next()).is_pending());
        drop(stream);
        assert!(writer.write_all(b"data: {}\n\n").await.is_err());
    }

    #[test]
    fn gemini_legacy_calls_use_local_ids_without_sending_them_to_provider() {
        let response = parse_response(
            json!({"candidates":[{"content":{"parts":[{
            "functionCall":{"name":"lookup","args":{}}
        }]},"finishReason":"STOP"}]}),
            "m",
            Format::Gemini,
        )
        .unwrap();
        let message = response.choices[0].message.clone();
        let call = &message.tool_calls[0];
        assert!(call.id.starts_with("praxis-gemini-"));
        let result = Message::tool_result(call, "Done");
        let body = request_body(
            &ChatCompletionRequest::new("m", vec![message, result]),
            Format::Gemini,
            false,
        );
        assert!(
            body["contents"][1]["parts"][0]["functionResponse"]
                .get("id")
                .is_none()
        );
    }

    #[test]
    fn old_serialized_messages_remain_readable() {
        let message: Message =
            serde_json::from_value(json!({"role":"assistant","content":"Hello"})).unwrap();
        assert!(message.tool_calls.is_empty());
        assert!(message.provider_data.is_none());
        let body = request_body(
            &ChatCompletionRequest::new("m", vec![message]),
            Format::OpenAI,
            false,
        );
        assert!(body.get("tools").is_none());
        assert!(body["messages"][0].get("tool_calls").is_none());
    }
}
