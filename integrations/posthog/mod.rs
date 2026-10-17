use super::Operation;

pub fn operations() -> Vec<Operation> {
    vec![
        op!(
            "posthog",
            "events",
            "Read project events",
            "/api/projects/{project}/events/",
            ["event", "limit", "after"]
        ),
        op!(
            "posthog",
            "insights",
            "Read saved insights",
            "/api/projects/{project}/insights/",
            ["limit"]
        ),
    ]
}
