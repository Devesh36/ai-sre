use super::Operation;

pub fn operations() -> Vec<Operation> {
    vec![
        op!(
            "argocd",
            "applications",
            "Read deployment application status",
            "/api/v1/applications",
            ["selector"]
        ),
        op!(
            "argocd",
            "application",
            "Read application details",
            "/api/v1/applications/{application}",
            []
        ),
    ]
}
