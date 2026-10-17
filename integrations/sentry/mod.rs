use super::Operation;

pub fn operations() -> Vec<Operation> {
    vec![
        op!(
            "sentry",
            "issues",
            "Read organization issues",
            "/api/0/organizations/{organization}/issues/",
            ["query", "limit"]
        ),
        op!(
            "sentry",
            "events",
            "Read issue events",
            "/api/0/issues/{issue_id}/events/",
            ["full"]
        ),
        op!(
            "sentry",
            "projects",
            "Read organization projects",
            "/api/0/organizations/{organization}/projects/",
            []
        ),
    ]
}
