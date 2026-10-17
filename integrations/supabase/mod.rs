use super::Operation;

pub fn operations() -> Vec<Operation> {
    vec![op!(
        "supabase",
        "rows",
        "Read a configured table via REST",
        "/rest/v1/{table}",
        ["select", "limit", "order"]
    )]
}
