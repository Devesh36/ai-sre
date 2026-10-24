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
