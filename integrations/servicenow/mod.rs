use super::Operation;

pub fn operations() -> Vec<Operation> {
    vec![op!(
        "servicenow",
        "incidents",
        "Read incident records",
        "/api/now/table/incident",
        ["sysparm_query", "sysparm_limit"]
    )]
}
