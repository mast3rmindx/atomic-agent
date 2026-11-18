// Copyright (c) 2026 7-x Industries. All rights reserved.

use atomic_core::protocol::ChatMessage;
use atomic_core::session::{Session, SessionStore};
use tempfile::tempdir;

#[tokio::test]
async fn persists_and_reloads_a_session_transcript() {
    let directory = tempdir().unwrap();
    let store = SessionStore::new(directory.path());
    let session = Session::new("session-1", directory.path()).with_messages(vec![
        ChatMessage::user("hello"),
        ChatMessage::assistant("hi"),
    ]);

    store.save(&session).await.unwrap();
    let loaded = store.load("session-1").await.unwrap();

    assert_eq!(loaded.messages, session.messages);
    assert_eq!(loaded.workspace, directory.path());
}
