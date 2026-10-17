use super::Operation;

pub fn operations() -> Vec<Operation> {
    vec![op!(
        "discord",
        "messages",
        "Read channel messages",
        "/api/v10/channels/{channel}/messages",
        ["limit", "before"]
    )]
}
