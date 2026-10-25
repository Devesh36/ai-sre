use super::terminal::Ui;
use crate::domain::review::{Changes, Review};

pub fn print_changes(changes: &Changes) {
    let ui = Ui::detect();
    ui.heading("Evidence changes");
    ui.text(&changes.reason);
    if changes.available {
        ui.text(&format!("Added: {}", ids(&changes.added)));
        ui.text(&format!("Not repeated: {}", ids(&changes.not_repeated)));
        for repeated in &changes.repeated {
            ui.text(&format!(
                "Same retained payload: {} → {}",
                ids(&repeated.before),
                ids(&repeated.after)
            ));
        }
    }
    ui.note("Not repeated does not mean recovered. Queries and source coverage may differ.");
}

pub fn print_review(review: &Review) {
    let ui = Ui::detect();
    ui.heading("Investigation review");
    ui.text(&format!(
        "{} observations · {} distinct retained payloads · {} repeats · {} context items",
        review.observations,
        review.distinct_payloads,
        review.repeated_payloads,
        review.context_items
    ));
    print_changes(&review.changes);
    for finding in &review.findings {
        ui.heading(&finding.title);
        ui.text(&format!(
            "Distinct payloads: {} · Source labels: {}",
            finding.distinct_payloads, finding.source_count
        ));
        ui.text(&format!(
            "Latest-run observations: {}",
            ids(&finding.current_observations)
        ));
        for concern in &finding.concerns {
            ui.text(&format!("• {concern}"));
        }
    }
    ui.heading("Checks for the next operator");
    for check in &review.checks {
        ui.text(&format!("• {check}"));
    }
    ui.heading("Interpretation limits");
    for limit in &review.limitations {
        ui.note(limit);
    }
}

fn ids(values: &[String]) -> String {
    if values.is_empty() {
        "none".into()
    } else {
        values.join(" · ")
    }
}

pub fn print_recall(recall: &crate::domain::recall::Recall) {
    let ui = Ui::detect();
    ui.heading("Past incident recall");
    ui.text(&recall.reason);
    if recall.search_truncated {
        ui.note("Search limited to the 100 most recently opened eligible cases for this service.");
    }
    for check in &recall.checks {
        ui.note(check);
    }
    for case in &recall.cases {
        ui.heading(&case.title);
        ui.text(&format!(
            "{} · {:?} · matched run {}",
            case.incident_id, case.status, case.matched_run
        ));
        ui.text(&format!(
            "{} distinct exact payload matches · {} shared symptom rules",
            case.exact_matches.len(),
            case.shared_symptoms.len()
        ));
        for (kind, matches) in [
            ("Exact payload", &case.exact_matches),
            ("Symptom", &case.shared_symptoms),
        ] {
            for m in matches {
                ui.text(&format!(
                    "{kind}: {} · current [{}] ↔ past [{}]",
                    m.label,
                    ids(&m.current_ids),
                    ids(&m.past_ids)
                ));
            }
        }
        ui.text(&format!(
            "Current-only symptoms: {}",
            ids(&case.current_only_symptoms)
        ));
        ui.text(&format!(
            "Past-only symptoms: {}",
            ids(&case.past_only_symptoms)
        ));
        if case.operator_outcome_count > 0 {
            ui.note(&format!("Historical operator assessments for the matched run: {} (showing up to 5). Not proof about this incident.",case.operator_outcome_count));
            for o in &case.operator_outcomes {
                ui.text(&format!(
                    "{:?} · {} · {} · assessment {}\n{}",
                    o.verdict, o.actor, o.claim, o.id, o.rationale
                ));
            }
        }
        ui.note(&format!(
            "Inspect the past case: ai-sre show {}",
            case.incident_id
        ));
    }
    for limit in &recall.limitations {
        ui.note(limit);
    }
}

pub fn print_outcomes(history: &crate::domain::outcome::History) {
    let ui = Ui::detect();
    ui.heading("Operator outcomes");
    ui.note(history.notice);
    if history.records.is_empty() {
        ui.text("No operator assessments recorded.");
    }
    let superseded: std::collections::BTreeSet<_> = history
        .records
        .iter()
        .filter_map(|o| o.supersedes)
        .collect();
    for o in &history.records {
        ui.heading(&o.claim);
        ui.text(&format!(
            "{:?} · {} · {}{}",
            o.verdict,
            o.actor,
            o.recorded_at,
            if superseded.contains(&o.id) {
                " · superseded"
            } else {
                ""
            }
        ));
        ui.text(&format!(
            "Assessment {} · incident {} · run {}",
            o.id, o.incident_id, o.run_id
        ));
        ui.text(&o.rationale);
        for e in &o.evidence {
            ui.text(&format!(
                "[{}] {} · collected {} · SHA-256 {}",
                e.id, e.source, e.observed_at, e.payload_sha256
            ));
        }
        if let Some(id) = o.supersedes {
            ui.note(&format!(
                "Corrects assessment {id}; the original is retained."
            ));
        }
    }
}
