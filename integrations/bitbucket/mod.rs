use super::Operation;

pub fn operations() -> Vec<Operation> {
    vec![
        op!(
            "bitbucket",
            "commits",
            "Read repository commits",
            "/2.0/repositories/{workspace}/{repository}/commits",
            ["pagelen"]
        ),
        op!(
            "bitbucket",
            "pipelines",
            "Read repository pipelines",
            "/2.0/repositories/{workspace}/{repository}/pipelines",
            ["pagelen"]
        ),
    ]
}
