use super::Operation;

pub fn operations() -> Vec<Operation> {
    vec![
        op!(
            "grafana",
            "dashboards",
            "Search dashboards",
            "/api/search",
            ["query", "limit"]
        ),
        op!(
            "grafana",
            "annotations",
            "Read deployment and incident annotations",
            "/api/annotations",
            ["from", "to", "limit"]
        ),
        op!(
            "grafana",
            "health",
            "Read Grafana health",
            "/api/health",
            []
        ),
    ]
}
