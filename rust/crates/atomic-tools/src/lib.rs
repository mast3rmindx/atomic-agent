// Copyright (c) 2026 7-x Industries. All rights reserved.

#![forbid(unsafe_code)]

use async_trait::async_trait;
use atomic_core::{CoreError, Result, Tool, ToolContext, ToolDefinition, ToolResult};
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};
use tokio::process::Command;

pub struct FileReadTool;
pub struct FileWriteTool;
pub struct FileEditTool;
pub struct ShellCommandTool;
pub struct RipgrepTool;

#[async_trait]
impl Tool for FileReadTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new("file.read", "Read a UTF-8 text file inside the workspace")
            .with_schema(path_schema("path"))
    }

    async fn execute(&self, arguments: Value, context: &ToolContext) -> Result<ToolResult> {
        let path = workspace_path(context, required_string(&arguments, "path")?)?;
        let metadata = tokio::fs::metadata(&path)
            .await
            .map_err(|error| CoreError::Tool(error.to_string()))?;
        if metadata.len() > 2 * 1024 * 1024 {
            return Err(CoreError::Tool(
                "refusing to read files larger than 2 MiB".into(),
            ));
        }
        let contents = tokio::fs::read_to_string(path)
            .await
            .map_err(|error| CoreError::Tool(error.to_string()))?;
        Ok(ToolResult::success(contents))
    }
}

#[async_trait]
impl Tool for FileWriteTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "file.write",
            "Atomically write a UTF-8 text file inside the workspace",
        )
        .with_schema(json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "contents": { "type": "string" }
            },
            "required": ["path", "contents"],
            "additionalProperties": false
        }))
    }

    async fn execute(&self, arguments: Value, context: &ToolContext) -> Result<ToolResult> {
        let path = workspace_path(context, required_string(&arguments, "path")?)?;
        let contents = required_string(&arguments, "contents")?;
        if contents.len() > 2 * 1024 * 1024 {
            return Err(CoreError::Tool(
                "refusing to write content larger than 2 MiB".into(),
            ));
        }
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|error| CoreError::Tool(error.to_string()))?;
        }
        let temporary = path.with_extension("atomic-rs.tmp");
        tokio::fs::write(&temporary, contents)
            .await
            .map_err(|error| CoreError::Tool(error.to_string()))?;
        tokio::fs::rename(temporary, &path)
            .await
            .map_err(|error| CoreError::Tool(error.to_string()))?;
        Ok(ToolResult::success(format!(
            "wrote {}",
            display_relative(context, &path)
        )))
    }
}

#[async_trait]
impl Tool for FileEditTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "file.edit",
            "Replace one exact occurrence in a UTF-8 workspace file",
        )
        .with_schema(json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "old": { "type": "string", "minLength": 1 },
                "new": { "type": "string" }
            },
            "required": ["path", "old", "new"],
            "additionalProperties": false
        }))
    }

    async fn execute(&self, arguments: Value, context: &ToolContext) -> Result<ToolResult> {
        let path = workspace_path(context, required_string(&arguments, "path")?)?;
        let old = required_string(&arguments, "old")?;
        if old.is_empty() {
            return Err(CoreError::Tool("old text must not be empty".into()));
        }
        let new = required_string(&arguments, "new")?;
        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(|error| CoreError::Tool(error.to_string()))?;
        if content.matches(old).count() != 1 {
            return Err(CoreError::Tool("old text must occur exactly once".into()));
        }
        let replacement = content.replacen(old, new, 1);
        FileWriteTool
            .execute(
                json!({"path": required_string(&arguments, "path")?, "contents": replacement}),
                context,
            )
            .await
    }
}

#[async_trait]
impl Tool for ShellCommandTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "shell.run",
            "Run an approved executable with structured arguments",
        )
        .with_schema(json!({
            "type": "object",
            "properties": {
                "program": { "type": "string", "minLength": 1 },
                "args": { "type": "array", "items": { "type": "string" } }
            },
            "required": ["program", "args"],
            "additionalProperties": false
        }))
    }

    async fn execute(&self, arguments: Value, context: &ToolContext) -> Result<ToolResult> {
        let program = required_string(&arguments, "program")?;
        if Path::new(program).is_absolute() || program.contains(['/', '\\']) {
            return Err(CoreError::Tool(
                "program must be a PATH executable name".into(),
            ));
        }
        let args = arguments
            .get("args")
            .and_then(Value::as_array)
            .ok_or_else(|| CoreError::Tool("args must be an array of strings".into()))?
            .iter()
            .map(|value| value.as_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| CoreError::Tool("args must be an array of strings".into()))?;
        let output = Command::new(program)
            .args(args)
            .current_dir(&context.workspace)
            .output()
            .await
            .map_err(|error| CoreError::Tool(error.to_string()))?;
        let text = bounded_output(&output.stdout, &output.stderr);
        if output.status.success() {
            Ok(ToolResult::success(text))
        } else {
            Ok(ToolResult::error(format!(
                "exit {:?}\n{text}",
                output.status.code()
            )))
        }
    }
}

#[async_trait]
impl Tool for RipgrepTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new("search.ripgrep", "Search workspace text using ripgrep").with_schema(
            json!({
                "type": "object",
                "properties": { "query": { "type": "string", "minLength": 1 } },
                "required": ["query"],
                "additionalProperties": false
            }),
        )
    }

    async fn execute(&self, arguments: Value, context: &ToolContext) -> Result<ToolResult> {
        let query = required_string(&arguments, "query")?;
        if query.is_empty() {
            return Err(CoreError::Tool("query must not be empty".into()));
        }
        let output = Command::new("rg")
            .args([
                "--line-number",
                "--no-heading",
                "--color",
                "never",
                "--",
                query,
                ".",
            ])
            .current_dir(&context.workspace)
            .output()
            .await
            .map_err(|error| CoreError::Tool(format!("could not run ripgrep: {error}")))?;
        let text = bounded_output(&output.stdout, &output.stderr);
        match output.status.code() {
            Some(0) => Ok(ToolResult::success(text)),
            Some(1) => Ok(ToolResult::success(String::new())),
            _ => Ok(ToolResult::error(format!("ripgrep failed\n{text}"))),
        }
    }
}

fn required_string<'a>(arguments: &'a Value, field: &str) -> Result<&'a str> {
    arguments
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| CoreError::Tool(format!("{field} must be a string")))
}

fn path_schema(field: &str) -> Value {
    json!({
        "type": "object",
        "properties": { field: { "type": "string" } },
        "required": [field],
        "additionalProperties": false
    })
}

fn workspace_path(context: &ToolContext, input: &str) -> Result<PathBuf> {
    let relative = Path::new(input);
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(CoreError::Tool(
            "paths must stay inside the workspace".into(),
        ));
    }
    Ok(context.workspace.join(relative))
}

fn display_relative(context: &ToolContext, path: &Path) -> String {
    path.strip_prefix(&context.workspace)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn bounded_output(stdout: &[u8], stderr: &[u8]) -> String {
    const MAX: usize = 64 * 1024;
    let mut bytes = Vec::with_capacity(stdout.len() + stderr.len());
    bytes.extend_from_slice(stdout);
    bytes.extend_from_slice(stderr);
    let truncated = bytes.len() > MAX;
    bytes.truncate(MAX);
    let mut output = String::from_utf8_lossy(&bytes).into_owned();
    if truncated {
        output.push_str("\n[output truncated at 64 KiB]");
    }
    output
}
