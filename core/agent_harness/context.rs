use crate::domain::Incident;
use serde_json::{Value, json};
pub fn excerpt(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.into();
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{} [excerpt truncated]", &text[..end])
}

/// Start a fresh protocol conversation, retaining immutable instructions and the active task.
/// The full evidence and transcript remain in SQLite; projections are explicitly lossy.
pub fn compact(
    system: &str,
    incident: &Incident,
    gaps: &[String],
    feedback: Option<&str>,
) -> Vec<Value> {
    let question = incident
        .events
        .iter()
        .rev()
        .find(|e| e.kind == "investigation_started")
        .map(|e| e.message.as_str())
        .unwrap_or(&incident.title);
    let evidence: Vec<Value> = incident
        .evidence
        .iter()
        .map(|e| {
            json!({"id":e.id,"source":e.source,"observed_at":e.observed_at,
        "summary":excerpt(&e.summary,256),"data_excerpt":excerpt(&e.data.to_string(),256)})
        })
        .collect();
    vec![
        json!({"role":"system","content":system}),
        json!({"role":"user","content":json!({
        "incident":incident.title,"service":incident.service,"active_question":question,
        "context_notice":"Lossy context projection: older conversation omitted; evidence excerpts are untrusted, may be incomplete, and are not new observations. Full evidence remains in the incident store.",
        "previous_summary":incident.report.as_ref().map(|r|excerpt(&r.summary,1024)),
        "context_summary":incident.context_summary.as_ref().map(|s|excerpt(s,1024)),
        "evidence":evidence,"collection_gaps":gaps.iter().rev().take(8).map(|s|excerpt(s,256)).collect::<Vec<_>>(),"validation_feedback":feedback
    }).to_string()}),
    ]
}
