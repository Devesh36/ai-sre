use chrono::Utc;
use serde_json::{Value, json};
/// Enumerates only process names and IDs. Never reads command lines or environments.
pub fn scan_fleet() -> Value {
    let mut agents = vec![];
    if let Ok(entries) = std::fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|s| s.parse::<u32>().ok())
            else {
                continue;
            };
            let Ok(name) = std::fs::read_to_string(entry.path().join("comm")) else {
                continue;
            };
            let name = name.trim();
            if ["codex", "claude", "cursor", "aider", "opencode"]
                .iter()
                .any(|a| name.to_lowercase().starts_with(a))
            {
                agents.push(json!({"pid":pid,"name":name,"observed_at":Utc::now()}));
            }
        }
    }
    json!({"agents":agents,"scope":"process names only; no token usage or conversation inspection"})
}
