use super::Operation;

pub fn operations() -> Vec<Operation> {
    vec![
        op!(
            "pagerduty",
            "incidents",
            "Read incidents",
            "/incidents",
            ["since", "until", "limit", "offset"]
        ),
        op!(
            "pagerduty",
            "services",
            "Read service configuration",
            "/services",
            ["limit", "offset"]
        ),
        op!(
            "pagerduty",
            "timeline",
            "Read incident log entries",
            "/incidents/{incident_id}/log_entries",
            ["limit"]
        ),
    ]
}
