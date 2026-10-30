mod support;
use axum::{Json, Router, body::to_bytes, extract::{Request, State}, http::StatusCode, response::{IntoResponse, Response}, routing::any};
use beweft_providers::*;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Fixture { requests: Vec<(String, String)> }

async fn provider(State(state): State<Arc<Mutex<Fixture>>>, request: Request) -> Response {
    let method = request.method().to_string();
    let path = request.uri().path().to_owned();
    let bytes = to_bytes(request.into_body(), 1_000_000).await.unwrap();
    let body = String::from_utf8_lossy(&bytes).to_string();
    let mut state = state.lock().unwrap();
    state.requests.push((format!("{} {}", method, path), body));
    let v = match path.as_str() {
        "/v1.0/account-1/threads" => json!({"id":"container-1"}),
        "/v1.0/account-1/threads_publish" => json!({"id":"threads-reply-1"}),
        _ => return (StatusCode::NOT_FOUND, Json(json!({"error":"missing"}))).into_response(),
    };
    Json(v).into_response()
}

#[tokio::test]
async fn threads_text_reply_happy_path() {
    assert_eq!(std::env::var("BEWEFT_REQUIRE_PROVIDER_FIXTURES").as_deref(), Ok("true"));
    assert_eq!(std::env::var("BEWEFT_PROVIDER_TEST_SANDBOX").as_deref(), Ok("1"));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let state = Arc::new(Mutex::new(Fixture::default()));
    let app = Router::new().fallback(any(provider)).with_state(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let adapter = Adapter::fixture(&origin).unwrap();
    let c = Channel {
        identity: Identity { parent: None, id: "account-1".into(), name: "Fixture".into(), username: None },
        tokens: Tokens { access_token: "fixture-only".into(), refresh_token: None, expires_at: None, scopes: vec![], scopes_known: false },
        app: AppConfig { provider: "threads".into(), client_id: "fixture-client".into(), client_secret: "fixture-secret".into(), instance_url: None },
    };
    let item = InboxItem { id: "post-1".into(), parent_id: None, author: "Fixture".into(), text: "hello".into(), url: None, created_at: None, reply_context: None };
    let step = adapter.reply(&c, &item, "Reply text", "request-1").await.unwrap();
    match step {
        Step::Published { external_id, .. } => assert_eq!(external_id, "threads-reply-1"),
        _ => panic!("expected published"),
    }
    let requests = state.lock().unwrap().requests.clone();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].0, "POST /v1.0/account-1/threads");
    assert!(requests[0].1.contains("media_type=TEXT"));
    assert!(requests[0].1.contains("text=Reply+text") || requests[0].1.contains("text=Reply%20text") || requests[0].1.contains("text=Reply text"));
    assert!(requests[0].1.contains("reply_to_id=post-1"));
    assert_eq!(requests[1].0, "POST /v1.0/account-1/threads_publish");
    assert!(requests[1].1.contains("creation_id=container-1"));
    server.abort();
    let _ = server.await;
}