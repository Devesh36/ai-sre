use super::Operation;

pub fn operations() -> Vec<Operation> {
    vec![
        op!(
            "opsgenie",
            "alerts",
            "Read alerts",
            "/v2/alerts",
            ["query", "limit", "offset"]
        ),
        op!(
            "opsgenie",
            "incident",
            "Read incident details",
            "/v1/incidents/{incident_id}",
            []
        ),
    ]
}
