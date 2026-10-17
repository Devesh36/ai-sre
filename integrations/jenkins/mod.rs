use super::Operation;

pub fn operations() -> Vec<Operation> {
    vec![
        op!(
            "jenkins",
            "builds",
            "Read job build history",
            "/job/{job}/api/json",
            ["tree"]
        ),
        op!(
            "jenkins",
            "build",
            "Read build details",
            "/job/{job}/{build}/api/json",
            []
        ),
    ]
}
