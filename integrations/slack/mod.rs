use super::Operation;

pub fn operations() -> Vec<Operation> {
    vec![
        op!(
            "slack",
            "history",
            "Read channel history",
            "/api/conversations.history",
            ["channel", "oldest", "latest", "limit"]
        ),
        op!(
            "slack",
            "thread",
            "Read incident discussion",
            "/api/conversations.replies",
            ["channel", "ts", "limit"]
        ),
    ]
}
