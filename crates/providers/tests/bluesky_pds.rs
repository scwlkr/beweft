//! Bluesky entryway-to-PDS discovery fixtures; no live accounts.
mod support;
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::any,
};
use beweft_providers::*;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Fixture {
    session_requests: usize,
}

async fn provider(State(state): State<Arc<Mutex<Fixture>>>, request: Request) -> Response {
    let path = request.uri().path().to_owned();
    let bytes = to_bytes(request.into_body(), 10_000).await.unwrap();
    let _body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    if path == "/xrpc/com.atproto.server.createSession" {
        state.lock().unwrap().session_requests += 1;
        return Json(json!({
            "did":"did:plc:fixture",
            "handle":"fixture.bsky.social",
            "accessJwt":"bluesky-access-fixture",
            "refreshJwt":"bluesky-refresh-fixture",
            "active":true,
            "didDoc":{"id":"did:plc:fixture","service":[{"id":"#atproto_pds","serviceEndpoint":"https://pds.example"}]}
        }))
        .into_response();
    }
    StatusCode::NOT_FOUND.into_response()
}

#[tokio::test]
async fn bluesky_connect_refuses_a_different_discovered_pds_without_moving_grants() {
    assert_eq!(
        std::env::var("BEWEFT_REQUIRE_PROVIDER_FIXTURES").as_deref(),
        Ok("true")
    );
    assert_eq!(
        std::env::var("BEWEFT_PROVIDER_TEST_SANDBOX").as_deref(),
        Ok("1")
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let state = Arc::new(Mutex::new(Fixture::default()));
    let server = tokio::spawn(
        axum::serve(
            listener,
            Router::new().fallback(any(provider)).with_state(state.clone()),
        )
        .into_future(),
    );
    let adapter = Adapter::fixture(&origin).unwrap();
    let app = AppConfig {
        provider: "bluesky".into(),
        client_id: "".into(),
        client_secret: "".into(),
        instance_url: Some("https://bsky.social".into()),
        organization_access: false,
    };
    let err = match adapter
        .connect_bluesky(&app, "fixture.bsky.social", "password")
        .await
    {
        Err(err) => err,
        Ok(_) => panic!("expected PDS mismatch error"),
    };
    assert!(err.message.contains("different PDS"));
    assert!(err.message.contains("https://pds.example"));
    assert_eq!(state.lock().unwrap().session_requests, 1);
    server.abort();
}

#[test]
fn bluesky_pds_origin_is_discovered_from_did_document() {
    let value = json!({"didDoc":{"service":[{"id":"#atproto_pds","serviceEndpoint":"https://pds.example/"},{"id":"#atproto_labeler","serviceEndpoint":"https://labeler.example"}]}});
    assert_eq!(
        bluesky_pds_origin(&value).as_deref(),
        Some("https://pds.example")
    );
    assert!(bluesky_pds_origin(&json!({})).is_none());
}