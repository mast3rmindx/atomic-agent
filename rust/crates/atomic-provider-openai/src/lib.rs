// Copyright (c) 2026 7-x Industries. All rights reserved.

#![forbid(unsafe_code)]

use async_trait::async_trait;
use atomic_core::protocol::Role;
use atomic_core::{
    ChatMessage, Completion, CoreError, LlmProvider, Result, ToolCall, ToolDefinition,
};
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct OpenAiCompatibleConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
}

pub struct OpenAiCompatibleProvider {
    client: Client,
    config: OpenAiCompatibleConfig,
}

impl OpenAiCompatibleProvider {
    pub fn new(config: OpenAiCompatibleConfig) -> Result<Self> {
        if config.base_url.trim().is_empty() || config.model.trim().is_empty() {
            return Err(CoreError::Provider(
                "base URL and model are required".into(),
            ));
        }
        let client = Client::builder()
            .build()
            .map_err(|error| CoreError::Provider(error.to_string()))?;
        Ok(Self { client, config })
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatibleProvider {
    async fn complete(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<Completion> {
        let body = json!({
            "model": self.config.model,
            "messages": messages.iter().map(to_openai_message).collect::<Vec<_>>(),
            "tools": tools.iter().map(to_openai_tool).collect::<Vec<_>>(),
            "stream": true
        });
        let url = format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        );
        let mut request = self.client.post(url).json(&body);
        if let Some(api_key) = &self.config.api_key {
            request = request.bearer_auth(api_key);
        }
        let response = request
            .send()
            .await
            .map_err(|error| CoreError::Provider(error.to_string()))?;
        let response = response
            .error_for_status()
            .map_err(|error| CoreError::Provider(error.to_string()))?;
        let mut stream = response.bytes_stream();
        let mut pending = String::new();
        let mut content = String::new();
        let mut calls = BTreeMap::<usize, ToolCallAccumulator>::new();
        while let Some(chunk) = stream.next().await {
            let bytes = chunk.map_err(|error| CoreError::Provider(error.to_string()))?;
            pending.push_str(&String::from_utf8_lossy(&bytes));
            while let Some(boundary) = pending.find("\n\n") {
                let event = pending[..boundary].to_owned();
                pending.drain(..boundary + 2);
                if let Some(data) = event.lines().find_map(|line| line.strip_prefix("data: ")) {
                    if data == "[DONE]" {
                        return build_completion(content, calls);
                    }
                    absorb_delta(data, &mut content, &mut calls)?;
                }
            }
        }
        build_completion(content, calls)
    }
}

fn to_openai_message(message: &ChatMessage) -> Value {
    let role = match message.role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    };
    let mut value = json!({ "role": role, "content": message.content });
    if !message.tool_calls.is_empty() {
        value["tool_calls"] = Value::Array(
            message
                .tool_calls
                .iter()
                .map(|call| {
                    json!({
                        "id": call.id,
                        "type": "function",
                        "function": { "name": call.name, "arguments": call.arguments.to_string() }
                    })
                })
                .collect(),
        );
    }
    if let Some(id) = &message.tool_call_id {
        value["tool_call_id"] = Value::String(id.clone());
    }
    value
}

fn to_openai_tool(tool: &ToolDefinition) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.parameters
        }
    })
}

#[derive(Default)]
struct ToolCallAccumulator {
    id: String,
    name: String,
    arguments: String,
}

fn absorb_delta(
    data: &str,
    content: &mut String,
    calls: &mut BTreeMap<usize, ToolCallAccumulator>,
) -> Result<()> {
    let value: Value = serde_json::from_str(data)
        .map_err(|error| CoreError::Provider(format!("invalid SSE JSON: {error}")))?;
    let Some(delta) = value.pointer("/choices/0/delta") else {
        return Ok(());
    };
    if let Some(text) = delta.get("content").and_then(Value::as_str) {
        content.push_str(text);
    }
    let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) else {
        return Ok(());
    };
    for call in tool_calls {
        let index = call
            .get("index")
            .and_then(Value::as_u64)
            .ok_or_else(|| CoreError::Provider("tool call delta had no index".into()))?
            as usize;
        let accumulator = calls.entry(index).or_default();
        if let Some(id) = call.get("id").and_then(Value::as_str) {
            accumulator.id.push_str(id);
        }
        if let Some(name) = call.pointer("/function/name").and_then(Value::as_str) {
            accumulator.name.push_str(name);
        }
        if let Some(arguments) = call.pointer("/function/arguments").and_then(Value::as_str) {
            accumulator.arguments.push_str(arguments);
        }
    }
    Ok(())
}

fn build_completion(
    content: String,
    calls: BTreeMap<usize, ToolCallAccumulator>,
) -> Result<Completion> {
    let tool_calls = calls
        .into_values()
        .map(|call| {
            let arguments = serde_json::from_str(&call.arguments).map_err(|error| {
                CoreError::Provider(format!("invalid streamed tool arguments: {error}"))
            })?;
            Ok(ToolCall::new(call.id, call.name, arguments))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Completion {
        content,
        tool_calls,
    })
}
