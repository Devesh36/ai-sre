use ai_sre::surfaces::{
    interactive_shell::editor::completions,
    shared::terminal::{clean, wrap},
};
use std::{
    io::Write,
    process::{Command, Stdio},
};
use unicode_width::UnicodeWidthStr;
#[test]
fn terminal_text_cannot_inject_escape_sequences_and_wraps_wide_text() {
    let malicious = "normal\x1b[2J\x1b]52;c;secret\x07done\r\x08\u{202e}";
    assert_eq!(clean(malicious), "normaldone");
    assert_eq!(clean("before\x1b]0;title\x1b\\after"), "beforeafter");
    for width in [4, 20, 36, 76] {
        let lines = wrap(
            "Unicode 中文 🚀 findings https://example.test/very-long-unbroken-path\nnext step",
            width,
        );
        assert!(lines.iter().all(|line| line.width() <= width));
        assert!(
            lines
                .join("")
                .contains("https://example.test/very-long-unbroken-path")
        );
    }
}
#[test]
fn completion_replaces_only_the_command_and_never_guesses_arguments() {
    let candidates = completions("/serv");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].replacement, "/service ");
    assert!(completions("/service prod").is_empty());
    assert!(completions("normal question").is_empty());
    assert!(completions("/").len() > 10);
}
#[test]
fn piped_cli_json_is_clean_and_noninteractive_default_does_not_open_a_database() {
    let dir = tempfile::tempdir().unwrap();
    let binary = env!("CARGO_BIN_EXE_ai-sre");
    let help = Command::new(binary)
        .current_dir(dir.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("Usage:"));
    assert!(!dir.path().join(".ai-sre").exists());
    let output = Command::new(binary)
        .current_dir(dir.path())
        .args([
            "ask",
            "Check availability",
            "--service",
            "checkout",
            "--json",
        ])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let incident: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(incident["service"], "checkout");
    assert!(!output.stdout.contains(&27));
}
#[test]
fn plain_shell_recovers_command_errors_switches_services_and_redacts_history() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ai-sre"))
        .current_dir(dir.path())
        .args(["cli", "--service", "checkout"])
        .env("TERM", "dumb")
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"/resume invalid\n/service billing\nCheck latency token=hidden-value\n/history\n/clear-history\n/exit\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains("TERMINAL WORKSPACE")
            && text.contains("REPORT READY")
            && text.contains("Service: billing")
            && text.contains("Removed 1 history entries")
    );
    assert!(!text.contains("hidden-value") && !text.contains('\x1b'));
}
