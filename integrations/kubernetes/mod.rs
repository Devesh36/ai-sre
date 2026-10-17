use super::Operation;

pub fn operations() -> Vec<Operation> {
    vec![
        op!(
            "kubernetes",
            "pods",
            "List namespace pods",
            "/api/v1/namespaces/{namespace}/pods",
            ["labelSelector", "limit"]
        ),
        op!(
            "kubernetes",
            "events",
            "List namespace events",
            "/api/v1/namespaces/{namespace}/events",
            ["fieldSelector", "limit"]
        ),
        op!(
            "kubernetes",
            "deployments",
            "List namespace deployments",
            "/apis/apps/v1/namespaces/{namespace}/deployments",
            ["labelSelector", "limit"]
        ),
        op!(
            "kubernetes",
            "nodes",
            "List node health and capacity",
            "/api/v1/nodes",
            ["limit"]
        ),
    ]
}
