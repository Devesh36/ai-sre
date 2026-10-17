use super::Operation;

pub fn operations() -> Vec<Operation> {
    vec![
        op!(
            "opensearch",
            "health",
            "Read cluster health",
            "/_cluster/health",
            []
        ),
        Operation {
            kind: "opensearch",
            name: "search",
            description: "Search a configured index",
            path: "/{index}/_search",
            query: &[],
            search_body: true,
        },
    ]
}
