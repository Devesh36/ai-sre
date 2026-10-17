use crate::domain::{Incident, Status};
use anyhow::Result;
pub fn print_incident(i: &Incident, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(i)?);
        return Ok(());
    }
    println!("\n{}\n{} · {:?}\n", i.title, i.id, i.status);
    if let Some(report) = &i.report {
        println!("{}", report.summary);
        for f in &report.findings {
            println!(
                "\n• {} [{:?}] ({})\n  {}",
                f.title,
                f.confidence,
                f.evidence_ids.join(", "),
                f.explanation
            );
        }
        for step in &report.next_steps {
            println!("→ {step}");
        }
        for gap in &report.gaps {
            println!("Gap: {gap}");
        }
    }
    if i.status == Status::Failed
        && let Some(e) = i.events.last()
    {
        println!("Failure: {}", e.message);
    }
    println!(
        "\n{} evidence items · {} model calls · {} input / {} output tokens",
        i.evidence.len(),
        i.usage.model_calls,
        i.usage.input_tokens,
        i.usage.output_tokens
    );
    Ok(())
}
