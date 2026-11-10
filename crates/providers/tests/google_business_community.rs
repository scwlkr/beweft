mod support;
use axum::{Json, Router, body::to_bytes, extract::{Request, State}, http::StatusCode, response::{IntoResponse, Response}, routing::any};
use beweft_providers::*;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Fixture {
    requests: Vec<(String, String)>,
}

async fn provider(State(state): State<Arc<Mutex<Fixture>>>, request: Request) -> Response {
    let method = request.method().to_string();
    let path = request.uri().path().to_owned();
    let bytes = to_bytes(request.into_body(), 100_000).await.unwrap();
    let body = String::from_utf8_lossy(&bytes).to_string();
    let mut state = state.lock().unwrap();
    state.requests.push((format!("{} {}", method, path), body));
    match (method.as_str(), path.as_str()) {
        ("GET", "/v4/accounts/1/locations/10/reviews") => Json(json!({
            "reviews": [{
                "name": "accounts/1/locations/10/reviews/review1",
                "reviewer": {"displayName": "Reviewer"},
                "comment": "Great service",
                "createTime": "2026-11-01T10:00:00Z",
                "reviewReplyUrl": "https://example.invalid/review"
            }]
        })).into_response(),
        ("PUT", "/v4/accounts/1/locations/10/reviews/review1/reply") => {
            Json(json!({"reply": {"comment": "Thanks", "updateTime": "2026-11-02T10:00:00Z"}})).into_response()
        }
        ("POST", "/v4/accounts/1/locations/10/localPosts:reportInsights") => {
            Json(json!({
                "name": "accounts/1/locations/10",
                "localPostMetrics": [{
                    "localPostName": "accounts/1/locations/10/localPosts/post1",
                    "metricValues": [
                        {"metric": "LOCAL_POST_VIEWS_SEARCH", "totalValue": {"value": "42"}},
                        {"metric": "LOCAL_POST_ACTIONS_CALL_TO_ACTION", "totalValue": {"value": "7"}}
                    ]
                }],
                "timeZone": "UTC"
            })).into_response()
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

fn channel() -> Channel {
    Channel {
        app: AppConfig {
            provider: "google_business".into(),
            client_id: "fixture".into(),
            client_secret: "fixture".into(),
            instance_url: None,
            organization_access: false,
        },
        tokens: Tokens {
            access_token: "fixture".into(),
            refresh_token: None,
            expires_at: None,
            scopes: vec!["https://www.googleapis.com/auth/business.manage".into()],
            scopes_known: true,
        },
        identity: Identity {
            id: "locations/10".into(),
            parent: Some("accounts/1".into()),
            name: "One".into(),
            username: None,
        },
    }
}

#[tokio::test]
async fn google_business_reviews_reply_and_post_insights() {
    assert_eq!(std::env::var("BEWEFT_REQUIRE_PROVIDER_FIXTURES").as_deref(), Ok("true"));
    assert_eq!(std::env::var("BEWEFT_PROVIDER_TEST_SANDBOX").as_deref(), Ok("1"));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let state = Arc::new(Mutex::new(Fixture::default()));
    let server = tokio::spawn(axum::serve(listener, Router::new().fallback(any(provider)).with_state(state.clone())).into_future());
    let adapter = Adapter::fixture(&origin).unwrap();
    let c = channel();

    let items = adapter.inbox(&c, &[]).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "accounts/1/locations/10/reviews/review1");
    assert_eq!(items[0].author, "Reviewer");
    assert_eq!(items[0].text, "Great service");
    assert_eq!(items[0].url.as_deref(), Some("https://example.invalid/review"));

    let step = adapter.reply(&c, &items[0], "Thanks", "request-1").await.unwrap();
    match step {
        Step::Published { external_id, .. } => assert_eq!(external_id, "accounts/1/locations/10/reviews/review1"),
        _ => panic!("expected published"),
    }

    let metrics = adapter.metrics(&c, "accounts/1/locations/10/localPosts/post1").await.unwrap();
    assert!(metrics.iter().any(|m| m.name == "LOCAL_POST_VIEWS_SEARCH" && m.value == 42));
    assert!(metrics.iter().any(|m| m.name == "LOCAL_POST_ACTIONS_CALL_TO_ACTION" && m.value == 7));

    let requests = state.lock().unwrap().requests.clone();
    assert!(requests.iter().any(|(r, _)| r == "GET /v4/accounts/1/locations/10/reviews"));
    assert!(requests.iter().any(|(r, _)| r == "PUT /v4/accounts/1/locations/10/reviews/review1/reply"));
    assert!(requests.iter().any(|(r, _)| r == "POST /v4/accounts/1/locations/10/localPosts:reportInsights"));
    server.abort();
}
