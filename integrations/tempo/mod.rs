use super::Operation;

pub fn operations() -> Vec<Operation> {
    vec![
        op!(
            "tempo",
            "trace",
            "Read a distributed trace",
            "/api/traces/{trace_id}",
            []
        ),
        op!(
            "tempo",
            "search",
            "Search distributed traces",
            "/api/search",
            ["q", "start", "end", "limit"]
        ),
    ]
}
