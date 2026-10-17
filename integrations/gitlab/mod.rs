use super::Operation;

pub fn operations() -> Vec<Operation> {
    vec![
        op!(
            "gitlab",
            "pipelines",
            "Read project pipelines",
            "/api/v4/projects/{project}/pipelines",
            ["status", "per_page"]
        ),
        op!(
            "gitlab",
            "commits",
            "Read project commits",
            "/api/v4/projects/{project}/repository/commits",
            ["ref_name", "per_page"]
        ),
        op!(
            "gitlab",
            "deployments",
            "Read project deployments",
            "/api/v4/projects/{project}/deployments",
            ["per_page"]
        ),
    ]
}
