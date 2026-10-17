use super::Operation;

pub fn operations() -> Vec<Operation> {
    vec![
        op!(
            "betterstack",
            "incidents",
            "Read uptime incidents",
            "/api/v2/incidents",
            ["per_page"]
        ),
        op!(
            "betterstack",
            "monitors",
            "Read uptime monitors",
            "/api/v2/monitors",
            ["per_page"]
        ),
    ]
}
