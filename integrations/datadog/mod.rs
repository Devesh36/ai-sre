use super::Operation;

pub fn operations() -> Vec<Operation> {
    vec![
        op!(
            "datadog",
            "monitors",
            "Read monitor states",
            "/api/v1/monitor",
            ["name", "page", "page_size"]
        ),
        op!(
            "datadog",
            "events",
            "Read event timeline",
            "/api/v1/events",
            ["start", "end", "tags"]
        ),
        op!(
            "datadog",
            "metrics",
            "Query metrics",
            "/api/v1/query",
            ["from", "to", "query"]
        ),
    ]
}
