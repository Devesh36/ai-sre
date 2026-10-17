use super::Operation;

pub fn operations() -> Vec<Operation> {
    vec![
        op!(
            "vercel",
            "deployments",
            "Read deployments",
            "/v6/deployments",
            ["projectId", "teamId", "limit"]
        ),
        op!(
            "vercel",
            "deployment",
            "Read deployment status",
            "/v13/deployments/{deployment}",
            ["teamId"]
        ),
    ]
}
