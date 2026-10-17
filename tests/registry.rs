use ai_sre::{
    api::{self, App},
    config::{Config, Workflow},
    core::agent_harness::skills::Bundle,
    infrastructure::{
        skills_registry::{Registry, client},
        terminal::history::History,
    },
    store::Store,
    tools::Toolbox,
};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;

fn workflow(instruction: &str) -> Workflow {
    Workflow {
        name: "triage".into(),
        instructions: vec![instruction.into()],
        allowed_tools: vec!["memory_read".into()],
    }
}

#[test]
fn immutable_releases_activation_rollback_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.db");
    let registry = Registry::new(Store::open(&path).unwrap()).unwrap();
    let first = registry.publish(workflow("Collect evidence")).unwrap();
    assert_eq!(first.revision, 1);
    assert_eq!(
        registry
            .publish(workflow("Collect evidence"))
            .unwrap()
            .digest,
        first.digest
    );
    assert!(registry.active("triage").is_err());
    registry.activate("triage", &first.digest).unwrap();
    assert!(registry.rollback("triage").is_err());
    let second = registry.publish(workflow("Check dependencies")).unwrap();
    assert_eq!(second.revision, 2);
    assert_eq!(registry.active("triage").unwrap().digest, first.digest);
    registry.activate("triage", &second.digest).unwrap();
    assert!(registry.activate("triage", "missing").is_err());
    assert_eq!(registry.active("triage").unwrap().digest, second.digest);
    let reopened = Registry::new(Store::open(&path).unwrap()).unwrap();
    assert_eq!(reopened.rollback("triage").unwrap().digest, first.digest);
    assert_eq!(reopened.history("triage").unwrap().len(), 2);
    assert_eq!(
        reopened
            .get("triage", &second.digest)
            .unwrap()
            .workflow
            .instructions,
        vec!["Check dependencies"]
    );
    let mut bundle = Bundle::from(second);
    bundle.workflow.instructions.push("Tampered".into());
    assert!(bundle.validate().is_err());
    let mut invalid = workflow("Check");
    invalid.name = "../escape".into();
    assert!(registry.publish(invalid).is_err());
}

#[test]
fn concurrent_publishers_allocate_unique_revisions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.db");
    let registries: Vec<_> = (0..8)
        .map(|_| Registry::new(Store::open(&path).unwrap()).unwrap())
        .collect();
    let handles: Vec<_> = registries
        .into_iter()
        .enumerate()
        .map(|(i, registry)| {
            std::thread::spawn(move || {
                registry
                    .publish(workflow(&format!("Instruction {i}")))
                    .unwrap()
                    .revision
            })
        })
        .collect();
    let mut revisions: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    revisions.sort();
    assert_eq!(revisions, (1..=8).collect::<Vec<_>>());
}

#[test]
fn history_redacts_bounds_persists_and_isolates_services() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.db");
    let history = History::new(Store::open(&path).unwrap()).unwrap();
    let tools = Toolbox::new(Config::default()).unwrap();
    history
        .append(&tools, "billing", "token=super-secret check latency")
        .unwrap();
    assert!(
        !history.list("billing", 20).unwrap()[0]
            .text
            .contains("super-secret")
    );
    for i in 0..205 {
        history
            .append(&tools, "checkout", &format!("Question {i}"))
            .unwrap();
    }
    let reopened = History::new(Store::open(&path).unwrap()).unwrap();
    let entries = reopened.list("checkout", 200).unwrap();
    assert_eq!(entries.len(), 200);
    assert_eq!(entries[0].text, "Question 204");
    assert_eq!(entries[199].text, "Question 5");
    assert!(reopened.list("checkout", 201).is_err());
    assert!(
        reopened
            .append(&tools, "checkout", &"x".repeat(4001))
            .is_err()
    );
    assert_eq!(reopened.clear("checkout").unwrap(), 200);
    assert_eq!(reopened.list("billing", 20).unwrap().len(), 1);
}

#[tokio::test]
async fn authenticated_registry_download_etag_and_tamper_rejection() {
    let source_dir = tempfile::tempdir().unwrap();
    let source = Store::open(&source_dir.path().join("state.db")).unwrap();
    let registry = Registry::new(source.clone()).unwrap();
    let app = api::router(App::new(
        Config::default(),
        source,
        Some("test-token".into()),
    ));
    let denied = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/skills")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    let published = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/skills")
                .header("authorization", "Bearer test-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&workflow("Collect evidence")).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(published.status(), StatusCode::OK);
    let release = registry.history("triage").unwrap().remove(0);
    let activated = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/skills/triage/activate")
                .header("authorization", "Bearer test-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"digest":release.digest}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(activated.status(), StatusCode::OK);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://{}/api/skills/triage/release",
        listener.local_addr().unwrap()
    );
    // A local proxy supplies auth, keeping test credentials out of the process environment.
    let proxy = axum::Router::new().fallback(move |mut request: Request<Body>| {
        let app = app.clone();
        async move {
            request
                .headers_mut()
                .insert("authorization", "Bearer test-token".parse().unwrap());
            app.oneshot(request).await.unwrap()
        }
    });
    let server = tokio::spawn(async move {
        axum::serve(listener, proxy).await.unwrap();
    });
    let dest_dir = tempfile::tempdir().unwrap();
    let dest = Store::open(&dest_dir.path().join("state.db")).unwrap();
    let first = client::pull(&Config::default(), &dest, &url, None)
        .await
        .unwrap();
    assert_eq!(first["status"], "imported");
    assert_eq!(first["activated"], false);
    assert!(
        Registry::new(dest.clone())
            .unwrap()
            .active("triage")
            .is_err()
    );
    let second = client::pull(&Config::default(), &dest, &url, None)
        .await
        .unwrap();
    assert_eq!(second["status"], "unchanged");
    server.abort();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let mut bundle = Bundle::from(release);
    bundle.workflow.instructions.push("Injected".into());
    let bad = axum::Router::new().route(
        "/",
        axum::routing::get(move || {
            let value = serde_json::to_value(&bundle).unwrap();
            async move { axum::Json(value) }
        }),
    );
    let server = tokio::spawn(async move {
        axum::serve(listener, bad).await.unwrap();
    });
    assert!(
        client::pull(&Config::default(), &dest, &url, None)
            .await
            .is_err()
    );
    assert_eq!(
        Registry::new(dest)
            .unwrap()
            .history("triage")
            .unwrap()
            .len(),
        1
    );
    server.abort();
}
