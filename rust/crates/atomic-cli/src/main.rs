// Copyright (c) 2026 7-x Industries. All rights reserved.

use anyhow::Result;
use async_trait::async_trait;
use atomic_core::agent::{Agent, AgentConfig};
use atomic_core::session::{Session, SessionStore};
use atomic_core::{ApprovalGate, Event, EventSink, Tool, ToolCall};
use atomic_provider_openai::{OpenAiCompatibleConfig, OpenAiCompatibleProvider};
use atomic_tools::{FileEditTool, FileReadTool, FileWriteTool, RipgrepTool, ShellCommandTool};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "atomic-rs", about = "Phase 1 headless Atomic Agent core")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run one agent turn against an OpenAI-compatible endpoint such as vLLM.
    Run {
        #[arg(
            long,
            env = "ATOMIC_RS_ENDPOINT",
            default_value = "http://127.0.0.1:8000/v1"
        )]
        endpoint: String,
        #[arg(long, env = "ATOMIC_RS_MODEL")]
        model: String,
        #[arg(long, default_value = ".")]
        workspace: PathBuf,
        #[arg(long, default_value = "default")]
        session_id: String,
        #[arg(long)]
        api_key: Option<String>,
        /// Permit shell.run calls for this turn. The default is deny.
        #[arg(long)]
        approve_shell: bool,
        prompt: String,
    },
}

struct CliApprovalGate {
    approve_shell: bool,
}

#[async_trait]
impl ApprovalGate for CliApprovalGate {
    async fn approve(&self, call: &ToolCall) -> atomic_core::Result<bool> {
        Ok(call.name != "shell.run" || self.approve_shell)
    }
}

struct CliEvents;

impl EventSink for CliEvents {
    fn emit(&self, event: Event) {
        if let Event::ToolResult { tool, result, .. } = event {
            let status = if result.is_error { "error" } else { "ok" };
            eprintln!(
                "[{status}] {tool}: {}",
                result.output.lines().next().unwrap_or_default()
            );
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Run {
            endpoint,
            model,
            workspace,
            session_id,
            api_key,
            approve_shell,
            prompt,
        } => {
            run(
                endpoint,
                model,
                workspace,
                session_id,
                api_key,
                approve_shell,
                prompt,
            )
            .await
        }
    }
}

async fn run(
    endpoint: String,
    model: String,
    workspace: PathBuf,
    session_id: String,
    api_key: Option<String>,
    approve_shell: bool,
    prompt: String,
) -> Result<()> {
    let workspace = std::fs::canonicalize(&workspace)?;
    let state = workspace.join(".atomic-rs").join("sessions");
    let store = SessionStore::new(&state);
    let session_path = state.join(format!("{session_id}.json"));
    let mut session = if session_path.exists() {
        store.load(&session_id).await?
    } else {
        Session::new(&session_id, &workspace)
    };
    session
        .messages
        .push(atomic_core::ChatMessage::user(prompt));

    let provider = Arc::new(OpenAiCompatibleProvider::new(OpenAiCompatibleConfig {
        base_url: endpoint,
        model,
        api_key,
    })?);
    let tools: Vec<Arc<dyn Tool>> = vec![
        Arc::new(FileReadTool),
        Arc::new(FileWriteTool),
        Arc::new(FileEditTool),
        Arc::new(ShellCommandTool),
        Arc::new(RipgrepTool),
    ];
    let agent = Agent::with_context(
        provider,
        tools,
        Arc::new(CliApprovalGate { approve_shell }),
        Arc::new(CliEvents),
        AgentConfig { max_steps: 12 },
        atomic_core::ToolContext::new(&workspace),
    );
    let reply = agent.run(session.messages.clone()).await?;
    session
        .messages
        .push(atomic_core::ChatMessage::assistant(&reply));
    store.save(&session).await?;
    println!("{reply}");
    Ok(())
}
