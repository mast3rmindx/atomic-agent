// Copyright (c) 2026 7-x Industries. All rights reserved.

use async_trait::async_trait;
use atomic_core::agent::{Agent, AgentConfig};
use atomic_core::protocol::{ChatMessage, Completion, ToolCall, ToolDefinition};
use atomic_core::{ApprovalGate, Event, EventSink, LlmProvider, Tool, ToolContext, ToolResult};
use serde_json::json;
use std::sync::{Arc, Mutex};

struct ScriptedProvider {
    replies: Mutex<Vec<Completion>>,
}

#[async_trait]
impl LlmProvider for ScriptedProvider {
    async fn complete(
        &self,
        _messages: &[ChatMessage],
        _tools: &[ToolDefinition],
    ) -> atomic_core::Result<Completion> {
        Ok(self.replies.lock().unwrap().remove(0))
    }
}

struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new("echo", "Returns a value").with_schema(json!({
            "type": "object",
            "properties": { "value": { "type": "string" } },
            "required": ["value"],
            "additionalProperties": false
        }))
    }

    async fn execute(
        &self,
        arguments: serde_json::Value,
        _context: &ToolContext,
    ) -> atomic_core::Result<ToolResult> {
        Ok(ToolResult::success(arguments["value"].as_str().unwrap()))
    }
}

struct AllowAll;

#[async_trait]
impl ApprovalGate for AllowAll {
    async fn approve(&self, _call: &ToolCall) -> atomic_core::Result<bool> {
        Ok(true)
    }
}

#[derive(Default)]
struct RecordingEvents(Mutex<Vec<Event>>);

impl EventSink for RecordingEvents {
    fn emit(&self, event: Event) {
        self.0.lock().unwrap().push(event);
    }
}

#[tokio::test]
async fn executes_a_tool_call_then_returns_the_follow_up_reply() {
    let provider = Arc::new(ScriptedProvider {
        replies: Mutex::new(vec![
            Completion::tool_calls(vec![ToolCall::new(
                "call-1",
                "echo",
                json!({"value": "hello"}),
            )]),
            Completion::text("done"),
        ]),
    });
    let events = Arc::new(RecordingEvents::default());
    let agent = Agent::new(
        provider,
        vec![Arc::new(EchoTool)],
        Arc::new(AllowAll),
        events.clone(),
        AgentConfig { max_steps: 3 },
    );

    let reply = agent
        .run(vec![ChatMessage::user("say hello")])
        .await
        .unwrap();

    assert_eq!(reply, "done");
    assert!(events.0.lock().unwrap().iter().any(|event| matches!(
        event,
        Event::ToolResult { tool, result, .. } if tool == "echo" && result.output == "hello"
    )));
}
