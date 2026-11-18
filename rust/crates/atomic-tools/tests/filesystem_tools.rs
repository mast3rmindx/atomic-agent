// Copyright (c) 2026 7-x Industries. All rights reserved.

use atomic_core::{Tool, ToolContext};
use atomic_tools::{FileEditTool, FileReadTool, FileWriteTool};
use serde_json::json;
use tempfile::tempdir;

#[tokio::test]
async fn writes_reads_and_edits_only_files_inside_the_workspace() {
    let workspace = tempdir().unwrap();
    let context = ToolContext::new(workspace.path());
    let write = FileWriteTool;
    let read = FileReadTool;
    let edit = FileEditTool;

    write
        .execute(json!({"path": "note.txt", "contents": "draft"}), &context)
        .await
        .unwrap();
    edit.execute(
        json!({"path": "note.txt", "old": "draft", "new": "final"}),
        &context,
    )
    .await
    .unwrap();
    let result = read
        .execute(json!({"path": "note.txt"}), &context)
        .await
        .unwrap();

    assert_eq!(result.output, "final");
    let outside = write
        .execute(
            json!({"path": "../outside.txt", "contents": "no"}),
            &context,
        )
        .await;
    assert!(outside.is_err());
}
