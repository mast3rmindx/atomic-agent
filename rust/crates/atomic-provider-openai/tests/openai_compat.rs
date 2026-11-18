// Copyright (c) 2026 7-x Industries. All rights reserved.

use atomic_core::protocol::{ChatMessage, ToolDefinition};
use atomic_core::LlmProvider;
use atomic_provider_openai::{OpenAiCompatibleConfig, OpenAiCompatibleProvider};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[tokio::test]
async fn parses_streamed_openai_tool_call_deltas() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = vec![0; 4096];
        let received = socket.read(&mut request).await.unwrap();
        let request = String::from_utf8_lossy(&request[..received]);
        assert!(request.starts_with("POST /v1/chat/completions HTTP/1.1"));
        assert!(request.contains("\"stream\":true"));
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"file.read\",\"arguments\":\"{\\\"path\\\":\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"a.txt\\\"}\"}}]}}]}\n\n",
            "data: [DONE]\n\n"
        );
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        socket.write_all(response.as_bytes()).await.unwrap();
    });
    let provider = OpenAiCompatibleProvider::new(OpenAiCompatibleConfig {
        base_url: format!("http://{address}/v1"),
        model: "test-model".into(),
        api_key: None,
    })
    .unwrap();
    let tool = ToolDefinition::new("file.read", "Read a file").with_schema(json!({
        "type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]
    }));

    let completion = provider
        .complete(&[ChatMessage::user("read the file")], &[tool])
        .await
        .unwrap();

    assert_eq!(completion.tool_calls.len(), 1);
    assert_eq!(completion.tool_calls[0].name, "file.read");
    assert_eq!(completion.tool_calls[0].arguments, json!({"path": "a.txt"}));
    server.await.unwrap();
}
