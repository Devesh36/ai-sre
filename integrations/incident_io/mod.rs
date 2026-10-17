use super::Operation;

pub fn operations() -> Vec<Operation> {
    vec![op!(
        "incident_io",
        "incidents",
        "Read incidents",
        "/v2/incidents",
        ["page_size", "after"]
    )]
}
