// Copyright (c) 2026 7-x Industries. All rights reserved.

#![forbid(unsafe_code)]

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, CoreError>;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("provider error: {0}")]
    Provider(String),
    #[error("tool error: {0}")]
    Tool(String),
    #[error("agent ended without a reply")]
    NoReply,
    #[error("agent reached its {0}-step limit")]
    StepLimit(usize),
    #[error("session error: {0}")]
    Session(String),
}

pub mod protocol {
    use super::*;

    #[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
    #[serde(rename_all = "lowercase")]
    pub enum Role {
        System,
        User,
        Assistant,
        Tool,
    }

    #[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
    pub struct ChatMessage {
        pub role: Role,
        pub content: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pub tool_calls: Vec<ToolCall>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub tool_call_id: Option<String>,
    }

    impl ChatMessage {
        pub fn user(content: impl Into<String>) -> Self {
            Self::text(Role::User, content)
        }

        pub fn assistant(content: impl Into<String>) -> Self {
            Self::text(Role::Assistant, content)
        }

        pub fn tool(call_id: impl Into<String>, content: impl Into<String>) -> Self {
            Self {
                role: Role::Tool,
                content: content.into(),
                tool_calls: Vec::new(),
                tool_call_id: Some(call_id.into()),
            }
        }

        fn text(role: Role, content: impl Into<String>) -> Self {
            Self {
                role,
                content: content.into(),
                tool_calls: Vec::new(),
                tool_call_id: None,
            }
        }
    }

    #[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
    pub struct ToolCall {
        pub id: String,
        pub name: String,
        pub arguments: Value,
    }

    impl ToolCall {
        pub fn new(id: impl Into<String>, name: impl Into<String>, arguments: Value) -> Self {
            Self {
                id: id.into(),
                name: name.into(),
                arguments,
            }
        }
    }

    #[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
    pub struct ToolDefinition {
        pub name: String,
        pub description: String,
        pub parameters: Value,
    }

    impl ToolDefinition {
        pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
            Self {
                name: name.into(),
                description: description.into(),
                parameters: serde_json::json!({"type": "object", "properties": {}, "additionalProperties": false}),
            }
        }

        pub fn with_schema(mut self, parameters: Value) -> Self {
            self.parameters = parameters;
            self
        }
    }

    #[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
    pub struct Completion {
        pub content: String,
        pub tool_calls: Vec<ToolCall>,
    }

    impl Completion {
        pub fn text(content: impl Into<String>) -> Self {
            Self {
                content: content.into(),
                tool_calls: Vec::new(),
            }
        }

        pub fn tool_calls(tool_calls: Vec<ToolCall>) -> Self {
            Self {
                content: String::new(),
                tool_calls,
            }
        }
    }
}

pub use protocol::{ChatMessage, Completion, ToolCall, ToolDefinition};

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<Completion>;
}

#[derive(Clone, Debug, Serialize)]
pub struct ToolContext {
    pub workspace: PathBuf,
}

impl ToolContext {
    pub fn new(workspace: impl Into<PathBuf>) -> Self {
        Self {
            workspace: workspace.into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct ToolResult {
    pub output: String,
    pub is_error: bool,
}

impl ToolResult {
    pub fn success(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            is_error: false,
        }
    }

    pub fn error(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            is_error: true,
        }
    }
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn definition(&self) -> ToolDefinition;
    async fn execute(&self, arguments: Value, context: &ToolContext) -> Result<ToolResult>;
}

#[async_trait]
pub trait ApprovalGate: Send + Sync {
    async fn approve(&self, call: &ToolCall) -> Result<bool>;
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    TurnStarted,
    ToolCall {
        id: String,
        tool: String,
    },
    ToolResult {
        id: String,
        tool: String,
        result: ToolResult,
    },
    AssistantReply {
        text: String,
    },
    TurnFailed {
        message: String,
    },
}

pub trait EventSink: Send + Sync {
    fn emit(&self, event: Event);
}

pub struct NoopEventSink;

impl EventSink for NoopEventSink {
    fn emit(&self, _event: Event) {}
}

/// Commands accepted by a frontend adapter. The core deliberately does not
/// assume a terminal, webview, or a specific desktop framework.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CoreCommand {
    StartSession {
        session_id: String,
        workspace: PathBuf,
    },
    SendMessage {
        session_id: String,
        text: String,
    },
    ResolveApproval {
        call_id: String,
        approved: bool,
    },
    CancelTurn {
        session_id: String,
    },
}

pub mod agent {
    use super::*;
    use std::collections::HashMap;

    #[derive(Clone, Copy, Debug)]
    pub struct AgentConfig {
        pub max_steps: usize,
    }

    pub struct Agent {
        provider: Arc<dyn LlmProvider>,
        tools: HashMap<String, Arc<dyn Tool>>,
        approval: Arc<dyn ApprovalGate>,
        events: Arc<dyn EventSink>,
        config: AgentConfig,
        context: ToolContext,
    }

    impl Agent {
        pub fn new(
            provider: Arc<dyn LlmProvider>,
            tools: Vec<Arc<dyn Tool>>,
            approval: Arc<dyn ApprovalGate>,
            events: Arc<dyn EventSink>,
            config: AgentConfig,
        ) -> Self {
            Self::with_context(
                provider,
                tools,
                approval,
                events,
                config,
                ToolContext::new("."),
            )
        }

        pub fn with_context(
            provider: Arc<dyn LlmProvider>,
            tools: Vec<Arc<dyn Tool>>,
            approval: Arc<dyn ApprovalGate>,
            events: Arc<dyn EventSink>,
            config: AgentConfig,
            context: ToolContext,
        ) -> Self {
            let tools = tools
                .into_iter()
                .map(|tool| (tool.definition().name, tool))
                .collect();
            Self {
                provider,
                tools,
                approval,
                events,
                config,
                context,
            }
        }

        pub async fn run(&self, mut messages: Vec<ChatMessage>) -> Result<String> {
            self.events.emit(Event::TurnStarted);
            let definitions = self
                .tools
                .values()
                .map(|tool| tool.definition())
                .collect::<Vec<_>>();
            for _ in 0..self.config.max_steps {
                let completion = self.provider.complete(&messages, &definitions).await?;
                if completion.tool_calls.is_empty() {
                    if completion.content.is_empty() {
                        self.events.emit(Event::TurnFailed {
                            message: "provider returned no reply".into(),
                        });
                        return Err(CoreError::NoReply);
                    }
                    self.events.emit(Event::AssistantReply {
                        text: completion.content.clone(),
                    });
                    return Ok(completion.content);
                }

                messages.push(ChatMessage {
                    role: protocol::Role::Assistant,
                    content: completion.content,
                    tool_calls: completion.tool_calls.clone(),
                    tool_call_id: None,
                });
                for call in completion.tool_calls {
                    self.events.emit(Event::ToolCall {
                        id: call.id.clone(),
                        tool: call.name.clone(),
                    });
                    let result = match self.tools.get(&call.name) {
                        Some(tool) if self.approval.approve(&call).await? => {
                            match tool.execute(call.arguments.clone(), &self.context).await {
                                Ok(result) => result,
                                Err(error) => ToolResult::error(error.to_string()),
                            }
                        }
                        Some(_) => ToolResult::error("tool call denied by approval policy"),
                        None => ToolResult::error(format!("unknown tool: {}", call.name)),
                    };
                    self.events.emit(Event::ToolResult {
                        id: call.id.clone(),
                        tool: call.name.clone(),
                        result: result.clone(),
                    });
                    messages.push(ChatMessage::tool(call.id, result.output));
                }
            }
            self.events.emit(Event::TurnFailed {
                message: format!("step limit ({}) reached", self.config.max_steps),
            });
            Err(CoreError::StepLimit(self.config.max_steps))
        }
    }
}

pub mod session {
    use super::*;
    use std::fs::{self, OpenOptions};
    use std::io::Write;

    #[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
    pub struct Session {
        pub id: String,
        pub workspace: PathBuf,
        pub messages: Vec<ChatMessage>,
    }

    impl Session {
        pub fn new(id: impl Into<String>, workspace: impl Into<PathBuf>) -> Self {
            Self {
                id: id.into(),
                workspace: workspace.into(),
                messages: Vec::new(),
            }
        }

        pub fn with_messages(mut self, messages: Vec<ChatMessage>) -> Self {
            self.messages = messages;
            self
        }
    }

    pub struct SessionStore {
        root: PathBuf,
    }

    impl SessionStore {
        pub fn new(root: impl Into<PathBuf>) -> Self {
            Self { root: root.into() }
        }

        pub async fn save(&self, session: &Session) -> Result<()> {
            fs::create_dir_all(&self.root)
                .map_err(|error| CoreError::Session(error.to_string()))?;
            let bytes = serde_json::to_vec_pretty(session)
                .map_err(|error| CoreError::Session(error.to_string()))?;
            let destination = self.path_for(&session.id)?;
            let temporary = destination.with_extension("json.tmp");
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&temporary)
                .map_err(|error| CoreError::Session(error.to_string()))?;
            restrict_file_permissions(&file)?;
            file.write_all(&bytes)
                .map_err(|error| CoreError::Session(error.to_string()))?;
            file.sync_all()
                .map_err(|error| CoreError::Session(error.to_string()))?;
            fs::rename(temporary, destination)
                .map_err(|error| CoreError::Session(error.to_string()))
        }

        pub async fn load(&self, id: &str) -> Result<Session> {
            let path = self.path_for(id)?;
            let bytes = fs::read(path).map_err(|error| CoreError::Session(error.to_string()))?;
            serde_json::from_slice(&bytes).map_err(|error| CoreError::Session(error.to_string()))
        }

        fn path_for(&self, id: &str) -> Result<PathBuf> {
            if id.is_empty() || id.contains(['/', '\\']) || id.contains("..") {
                return Err(CoreError::Session("invalid session id".into()));
            }
            Ok(self.root.join(format!("{id}.json")))
        }
    }

    #[cfg(unix)]
    fn restrict_file_permissions(file: &std::fs::File) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|error| CoreError::Session(error.to_string()))
    }

    #[cfg(not(unix))]
    fn restrict_file_permissions(_file: &std::fs::File) -> Result<()> {
        Ok(())
    }
}
