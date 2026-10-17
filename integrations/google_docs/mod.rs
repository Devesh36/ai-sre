use super::Operation;

pub fn operations() -> Vec<Operation> {
    vec![op!(
        "google_docs",
        "document",
        "Read a document",
        "/v1/documents/{document}",
        []
    )]
}
