use super::Operation;

pub fn operations() -> Vec<Operation> {
    vec![
        op!(
            "github",
            "commits",
            "Read recent repository commits",
            "/repos/{owner}/{repository}/commits",
            ["sha", "per_page"]
        ),
        op!(
            "github",
            "deployments",
            "Read repository deployments",
            "/repos/{owner}/{repository}/deployments",
            ["environment", "per_page"]
        ),
        op!(
            "github",
            "runs",
            "Read CI workflow runs",
            "/repos/{owner}/{repository}/actions/runs",
            ["status", "per_page"]
        ),
        op!(
            "github",
            "issues",
            "Read repository issues",
            "/repos/{owner}/{repository}/issues",
            ["state", "per_page"]
        ),
    ]
}
