use super::terminal::{Ui, accent_code};
use crate::domain::{Incident, Status};
use anyhow::Result;
pub fn print_incident(i: &Incident, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(i)?);
        return Ok(());
    }
    let ui = Ui::detect();
    println!();
    ui.rule();
    let status = match i.status {
        Status::Triaged => "REPORT READY",
        Status::Failed => "INVESTIGATION FAILED",
        Status::Cancelled => "CANCELLED",
        Status::Resolved => "RESOLVED",
        Status::Investigating => "INVESTIGATING",
        Status::Open => "OPEN",
    };
    println!(
        "  {}",
        ui.style(
            status,
            if matches!(i.status, Status::Failed | Status::Cancelled) {
                "1;33"
            } else {
                accent_code()
            }
        )
    );
    ui.text(&i.title);
    ui.note(&format!("{} · {}", i.service, i.id));
    ui.rule();
    if let Some(report) = &i.report {
        ui.heading("Summary");
        ui.text(&report.summary);
        if !report.findings.is_empty() {
            ui.heading("Findings");
        }
        for (n, f) in report.findings.iter().enumerate() {
            ui.text(&format!(
                "{}. {} · {:?} support",
                n + 1,
                f.title,
                f.confidence
            ));
            ui.text(&f.explanation);
            ui.note(&format!("Evidence: {}", f.evidence_ids.join(" · ")));
            println!();
        }
        if !report.next_steps.is_empty() {
            ui.heading("Suggested next steps · review before acting");
            for (n, step) in report.next_steps.iter().enumerate() {
                ui.text(&format!("{}. {step}", n + 1));
            }
        }
        if !report.gaps.is_empty() {
            ui.heading("Still unknown");
            for gap in &report.gaps {
                ui.text(&format!("• {gap}"));
            }
        }
    }
    if i.status == Status::Failed
        && let Some(event) = i.events.last()
    {
        ui.error(&event.message);
    }
    if i.status == Status::Cancelled {
        ui.text("The run was cancelled. Completed evidence is retained; you can ask a follow-up to continue.");
    }
    println!();
    ui.rule();
    ui.note(&format!(
        "{} evidence items · {} tool calls · {} model calls\n{} input / {} output tokens",
        i.evidence.len(),
        i.usage.tool_calls,
        i.usage.model_calls,
        i.usage.input_tokens,
        i.usage.output_tokens
    ));
    Ok(())
}
