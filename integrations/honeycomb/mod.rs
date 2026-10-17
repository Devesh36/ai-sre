use super::Operation;

pub fn operations() -> Vec<Operation> {
    vec![
        op!("honeycomb", "datasets", "Read datasets", "/1/datasets", []),
        op!(
            "honeycomb",
            "triggers",
            "Read dataset triggers",
            "/1/triggers/{dataset}",
            []
        ),
    ]
}
