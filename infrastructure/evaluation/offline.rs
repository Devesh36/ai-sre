use crate::agent;
use anyhow::{Result, ensure};
pub fn evaluate() -> Result<()> {
    #[derive(serde::Deserialize)]
    struct Scenario {
        name: String,
        signal: String,
        expected: Option<String>,
    }
    let scenarios: Vec<Scenario> = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/scenarios.json"
    )))?;
    let total = scenarios.len();
    let mut passed = 0;
    for s in scenarios {
        let evidence = crate::domain::Evidence {
            id: "E1".into(),
            source: "evaluation".into(),
            observed_at: chrono::Utc::now(),
            summary: s.signal,
            data: serde_json::json!({}),
        };
        let report = agent::offline_report(&[evidence]);
        let ok = match s.expected {
            Some(title) => report.findings.iter().any(|f| f.title == title),
            None => report.findings.is_empty(),
        };
        println!("{} {}", if ok { "PASS" } else { "FAIL" }, s.name);
        passed += usize::from(ok);
    }
    println!("{passed}/{total} scenarios passed (offline symptom detection only)");
    ensure!(passed == total, "evaluation failed");
    Ok(())
}
