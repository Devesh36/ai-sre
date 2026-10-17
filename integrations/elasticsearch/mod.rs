use super::Operation;

pub fn operations() -> Vec<Operation> {
    vec![
        op!(
            "elasticsearch",
            "health",
            "Read cluster health",
            "/_cluster/health",
            []
        ),
        Operation {
            kind: "elasticsearch",
            name: "search",
            description: "Search a configured index",
            path: "/{index}/_search",
            query: &[],
            search_body: true,
        },
    ]
}
