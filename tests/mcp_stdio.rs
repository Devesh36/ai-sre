//! Real subprocess contract tests, including cancellation and malformed peers.
#![cfg(unix)]
use ai_sre::{
    config::Config,
    mcp::{Client, Server, Transport},
};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};

fn server(mode: &str, directory: &Path) -> Server {
    Server {
        name: "local".into(),
        mode: Transport::Stdio,
        command: Some("python3".into()),
        args: vec![
            format!("{}/tests/fixtures/mcp_peer.py", env!("CARGO_MANIFEST_DIR")),
            mode.into(),
            directory.display().to_string(),
            "$(touch should-not-exist); literal argument".into(),
        ],
        allowed_tools: vec!["inspect".into()],
        timeout_seconds: 2,
        ..Default::default()
    }
}
fn client(mode: &str, dir: &Path) -> Client {
    Client::new(server(mode, dir), Config::default().client().unwrap())
}
async fn wait_file(path: &Path) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("peer did not reach the expected phase");
}
async fn assert_reaped(directory: &Path) {
    let pid = std::fs::read_to_string(directory.join("pid")).unwrap();
    // Linux acceptance checks both termination and reaping (a zombie still has /proc).
    if cfg!(target_os = "linux") {
        tokio::time::timeout(Duration::from_secs(5), async {
            while Path::new(&format!("/proc/{pid}")).exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("MCP child was not reaped");
    }
}

#[test]
fn transport_configuration_is_explicit_and_http_remains_compatible() {
    let http: Server = serde_json::from_value(
        json!({"name":"remote","url":"https://example.test/mcp","allowed_tools":["inspect"]}),
    )
    .unwrap();
    http.validate().unwrap();
    assert_eq!(serde_json::to_value(http.mode).unwrap(), "streamable-http");
    assert_eq!(http.timeout_seconds, 30);
    let mut local = server("normal", Path::new("/tmp"));
    local.validate().unwrap();
    local.url = "https://example.test".into();
    assert!(local.validate().is_err());
    local.url.clear();
    local.timeout_seconds = 0;
    assert!(local.validate().is_err());
    local.timeout_seconds = 1;
    local.command = None;
    assert!(local.validate().is_err());
    assert!(
        serde_json::from_value::<Server>(
            json!({"name":"x","mode":"sse","allowed_tools":["inspect"]})
        )
        .is_err()
    );
}

#[tokio::test]
async fn local_session_handshakes_paginates_handles_server_requests_and_shares_clones() {
    let dir = tempfile::tempdir().unwrap();
    let client = client("normal", dir.path());
    assert!(client.call("inspect", json!({})).await.is_err());
    client.initialize().await.unwrap();
    assert_eq!(
        client.definitions()[0]["function"]["name"],
        "mcp_local_inspect"
    );
    assert!(client.call("unapproved", json!({})).await.is_err());
    let cloned = client.clone();
    let (a, b) = tokio::join!(
        client.call("inspect", json!({})),
        cloned.call("inspect", json!({}))
    );
    let a = a.unwrap();
    let b = b.unwrap();
    assert_eq!(a["structuredContent"]["pid"], b["structuredContent"]["pid"]);
    assert_eq!(a["content"][0]["text"], "ready λ");
    assert_eq!(
        a["structuredContent"]["args"][0],
        "$(touch should-not-exist); literal argument"
    );
    assert_eq!(a["structuredContent"]["no_color"], "1");
    assert_eq!(a["structuredContent"]["term"], "dumb");
    drop(client);
    drop(cloned);
    assert_reaped(dir.path()).await;
    let log = std::fs::read_to_string(dir.path().join("requests")).unwrap();
    let methods: Vec<Value> = log
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap()["method"].clone())
        .collect();
    assert_eq!(
        &methods[..4],
        &[
            json!("initialize"),
            json!("notifications/initialized"),
            json!("tools/list"),
            json!("tools/list")
        ]
    );
    assert_eq!(methods.iter().filter(|m| **m == "tools/call").count(), 2);
}

#[tokio::test]
async fn malformed_flooding_and_exited_peers_fail_closed_and_are_reaped() {
    for (mode, message) in [
        ("bad-json", "invalid JSON"),
        ("wrong-id", "mismatch"),
        ("oversized", "256 KiB"),
        ("flood", "64 messages"),
        ("byte-flood", "1 MiB"),
        ("exit", "closed stdout"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let client = client(mode, dir.path());
        client.initialize().await.unwrap();
        let error = client
            .call("inspect", json!({}))
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains(message), "{mode}: {error}");
        assert_reaped(dir.path()).await;
        assert!(
            client
                .call("inspect", json!({}))
                .await
                .unwrap_err()
                .to_string()
                .contains("initialize again")
        );
        // Explicit reinitialization starts a fresh negotiated session.
        client.initialize().await.unwrap();
        drop(client);
        assert_reaped(dir.path()).await;
    }
}

#[tokio::test]
async fn timeout_and_external_cancellation_kill_the_active_process() {
    for cancel in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let client = client("hang", dir.path());
        client.initialize().await.unwrap();
        let clone = client.clone();
        let task = tokio::spawn(async move { clone.call("inspect", json!({})).await });
        wait_file(&dir.path().join("called")).await;
        if cancel {
            task.abort();
            assert!(task.await.unwrap_err().is_cancelled());
        } else {
            assert!(
                task.await
                    .unwrap()
                    .unwrap_err()
                    .to_string()
                    .contains("timed out")
            );
        }
        assert_reaped(dir.path()).await;
        assert!(client.call("inspect", json!({})).await.is_err());
    }
}

#[tokio::test]
async fn discovery_rejects_mutating_tools_and_excessive_pages_without_leaking_children() {
    for mode in ["mutating", "pages"] {
        let dir = tempfile::tempdir().unwrap();
        let client = client(mode, dir.path());
        assert!(client.initialize().await.is_err());
        assert!(client.definitions().is_empty());
        assert_reaped(dir.path()).await;
    }
}

#[tokio::test]
async fn tool_errors_do_not_poison_the_protocol_session() {
    let dir = tempfile::tempdir().unwrap();
    let client = client("tool-error", dir.path());
    client.initialize().await.unwrap();
    for _ in 0..2 {
        assert!(
            client
                .call("inspect", json!({}))
                .await
                .unwrap_err()
                .to_string()
                .contains("tool reported an error")
        );
    }
    drop(client);
    assert_reaped(dir.path()).await;
}

#[test]
fn real_cli_uses_explicit_environment_mapping_and_redacts_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = Config {
        database: dir.path().join("incidents.db"),
        mcp: vec![server("normal", dir.path())],
        ..Config::default()
    };
    config.mcp[0]
        .env
        .insert("PEER_TOKEN".into(), "EXPLICIT_PEER_TOKEN".into());
    let path = dir.path().join("config.toml");
    std::fs::write(&path, toml::to_string(&config).unwrap()).unwrap();
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_ai-sre"))
        .args([
            "--config",
            path.to_str().unwrap(),
            "tool",
            "mcp_local_inspect",
            "{}",
        ])
        .env("EXPLICIT_PEER_TOKEN", "synthetic-mapped-credential")
        .env("UNMAPPED_TOKEN", "synthetic-private-credential")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let text = String::from_utf8(result.stdout).unwrap();
    assert!(!text.contains("synthetic-mapped-credential"));
    assert!(!text.contains("synthetic-private-credential"));
    let output: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        output[0]["data"]["structuredContent"]["credential"],
        "[REDACTED]"
    );
    assert!(output[0]["data"]["structuredContent"]["unmapped"].is_null());
    assert!(!String::from_utf8_lossy(&result.stderr).contains("private diagnostics"));
}
