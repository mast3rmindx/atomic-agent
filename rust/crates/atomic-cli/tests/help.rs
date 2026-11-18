// Copyright (c) 2026 7-x Industries. All rights reserved.

#[test]
fn exposes_a_run_command() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_atomic-rs"))
        .arg("--help")
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("run"));
}
