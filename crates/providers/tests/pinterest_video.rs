mod support;
use axum::{Json, Router, body::to_bytes, extract::{Request, State}, http::StatusCode, response::{IntoResponse, Response}, routing::any};
use beweft_providers::*;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Fixture {
    requests: Vec<(String, Value)>,
    session_origin: String,
}

async fn provider(State(state): State<Arc<Mutex<Fixture>>>, request: Request) -> Response {
    let path = request.uri().path().to_owned();
    let method = request.method().to_string();
    let bytes = to_bytes(request.into_body(), 20 * 1024 * 1024).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    let mut state = state.lock().unwrap();
    state.requests.push((format!("{method} {path}"), body.clone()));
    let v = match path.as_str() {
        "/v5/media" if method == "POST" => json!({
            "media_id": "media-1",
            "upload_url": "https://pinterest-media-upload.s3-accelerate.amazonaws.com/",
            "upload_parameters": {"key": "uploads/test", "policy": "policy"}
        }),
        "/" if method == "POST" => {
            return StatusCode::NO_CONTENT.into_response();
        }
        p if p.starts_with("/v5/media/") && method == "GET" => {
            json!({"media_id": "media-1", "status": "succeeded"})
        }
        "/v5/pins" => json!({"id": "video-pin-1"}),
        _ => {
            return (StatusCode::NOT_FOUND, Json(json!({"error":"fixture route missing"}))).into_response();
        }
    };
    Json(v).into_response()
}

fn channel() -> Channel {
    Channel {
        identity: Identity { parent: None, id: "account-1".into(), name: "Fixture".into(), username: None },
        tokens: Tokens { access_token: "fixture-only".into(), refresh_token: Some("fixture-refresh".into()), expires_at: None, scopes: vec![], scopes_known: false },
        app: AppConfig { provider: "pinterest".into(), client_id: "fixture-client".into(), client_secret: "fixture-secret".into(), instance_url: None, organization_access: false },
    }
}

#[tokio::test]
async fn pinterest_video_pin_registers_uploads_and_creates() {
    assert_eq!(std::env::var("BEWEFT_REQUIRE_PROVIDER_FIXTURES").as_deref(), Ok("true"));
    assert_eq!(std::env::var("BEWEFT_PROVIDER_TEST_SANDBOX").as_deref(), Ok("1"));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let state = Arc::new(Mutex::new(Fixture { session_origin: origin.clone(), ..Default::default() }));
    let app = Router::new().fallback(any(provider)).with_state(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let adapter = Adapter::fixture(&origin).unwrap();
    let c = channel();
    let mut p = Publication {
        key: "fixture-publication".into(),
        text: "Fixture content".into(),
        title: "Fixture title".into(),
        media: vec![Media {
            name: "fixture.mp4".into(),
            mime: "video/mp4".into(),
            source: MediaSource::Bytes(support::fixture_bytes(true)),
            public_url: None,
            alt_text: String::new(),
        }],
        options: json!({"board_id":"fixture-board","cover_image_url":"https://media.example/cover.jpg"}),
    };

    let mut cp = None;
    let mut completed = false;
    let mut published_id = String::new();
    for _ in 0..12 {
        match adapter.publish_step(&c, &p, cp.as_ref()).await.unwrap() {
            Step::Checkpoint(next) | Step::Wait(next) => cp = Some(next),
            Step::Published { external_id, .. } => {
                published_id = external_id;
                completed = true;
                break;
            }
        }
    }
    assert!(completed, "pinterest video pin must complete");
    assert_eq!(published_id, "video-pin-1");

    let f = state.lock().unwrap();
    assert!(f.requests.iter().any(|(r, _)| r == "POST /v5/media"));
    assert!(f.requests.iter().any(|(r, _)| r == "POST /"));
    assert!(f.requests.iter().any(|(r, _)| r.starts_with("GET /v5/media/")));
    let creates: Vec<_> = f.requests.iter().filter(|(r, _)| r == "POST /v5/pins").collect();
    assert_eq!(creates.len(), 1);
    assert_eq!(creates[0].1["media_source"]["source_type"], "video_id");
    assert_eq!(creates[0].1["media_source"]["media_id"], "media-1");
    assert_eq!(creates[0].1["media_source"]["cover_image_url"], "https://media.example/cover.jpg");
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn pinterest_video_pin_rejects_wrong_media() {
    assert_eq!(std::env::var("BEWEFT_REQUIRE_PROVIDER_FIXTURES").as_deref(), Ok("true"));
    assert_eq!(std::env::var("BEWEFT_PROVIDER_TEST_SANDBOX").as_deref(), Ok("1"));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let state = Arc::new(Mutex::new(Fixture { session_origin: origin.clone(), ..Default::default() }));
    let app = Router::new().fallback(any(provider)).with_state(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let adapter = Adapter::fixture(&origin).unwrap();
    let c = channel();
    let mut p = Publication {
        key: "k".into(),
        text: "t".into(),
        title: "T".into(),
        media: vec![],
        options: json!({"board_id":"fixture-board","cover_image_url":"https://media.example/cover.jpg"}),
    };
    let before = state.lock().unwrap().requests.len();
    assert!(adapter.publish_step(&c, &p, None).await.is_err());
    assert_eq!(state.lock().unwrap().requests.len(), before);

    let mut two = p.clone();
    two.media = vec![
        Media { name: "a.mp4".into(), mime: "video/mp4".into(), source: MediaSource::Bytes(support::fixture_bytes(true)), public_url: None, alt_text: String::new() },
        Media { name: "b.mp4".into(), mime: "video/mp4".into(), source: MediaSource::Bytes(support::fixture_bytes(true)), public_url: None, alt_text: String::new() },
    ];
    let before = state.lock().unwrap().requests.len();
    assert!(adapter.publish_step(&c, &two, None).await.is_err());
    assert_eq!(state.lock().unwrap().requests.len(), before);

    let mut image_without_cover = Publication {
        key: "k".into(),
        text: "t".into(),
        title: "T".into(),
        media: vec![Media { name: "v.mp4".into(), mime: "video/mp4".into(), source: MediaSource::Bytes(support::fixture_bytes(true)), public_url: None, alt_text: String::new() }],
        options: json!({"board_id":"fixture-board"}),
    };
    let before = state.lock().unwrap().requests.len();
    assert!(adapter.publish_step(&c, &image_without_cover, None).await.is_err());
    assert_eq!(state.lock().unwrap().requests.len(), before);

    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn pinterest_metrics_reads_pin_analytics() {
    assert_eq!(std::env::var("BEWEFT_REQUIRE_PROVIDER_FIXTURES").as_deref(), Ok("true"));
    assert_eq!(std::env::var("BEWEFT_PROVIDER_TEST_SANDBOX").as_deref(), Ok("1"));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let state = Arc::new(Mutex::new(Fixture { session_origin: origin.clone(), ..Default::default() }));
    // need analytics route; use inline provider
    let app = Router::new().fallback(any(|State(state): State<Arc<Mutex<Fixture>>>, req: Request| async move {
        let path = req.uri().path().to_owned();
        let method = req.method().to_string();
        let mut state = state.lock().unwrap();
        if path.starts_with("/v5/pins/") && path.ends_with("/analytics") && method == "GET" {
            return Json(json!({"pin-1": {"lifetime_metrics": {"IMPRESSION": 5, "SAVE": 2}}})).into_response();
        }
        (StatusCode::NOT_FOUND, Json(json!({"error":"missing"}))).into_response()
    })).with_state(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let adapter = Adapter::fixture(&origin).unwrap();
    let mut c = channel();
    c.tokens.scopes = vec!["pins:read".into()];
    c.tokens.scopes_known = true;
    let metrics = adapter.metrics(&c, "pin-1").await.unwrap();
    assert!(metrics.iter().any(|m| m.name == "IMPRESSION" && m.value == 5));
    assert!(metrics.iter().any(|m| m.name == "SAVE" && m.value == 2));
    server.abort();
    let _ = server.await;
}