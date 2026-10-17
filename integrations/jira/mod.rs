use super::Operation;

pub fn operations() -> Vec<Operation> {
    vec![
        op!(
            "jira",
            "issue",
            "Read an issue",
            "/rest/api/3/issue/{issue_key}",
            ["fields"]
        ),
        op!(
            "jira",
            "search",
            "Search issues with JQL",
            "/rest/api/3/search/jql",
            ["jql", "maxResults"]
        ),
    ]
}
