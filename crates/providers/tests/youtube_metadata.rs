//! YouTube upload metadata fixtures: no live YouTube calls or public uploads.
mod support;
use axum::{
    Router,
    body::to_bytes,
    extract::{Request, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::any,
};
use beweft_providers::*;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct Seen {
    method: String,
    uri: String,
    body: Value,
}

#[derive(Default)]
struct Fixture {
    requests: Vec<Seen>,
}

async fn provider(State(state): State<Arc<Mutex<Fixture>>>, request: Request) -> Response {
    let method = request.method().to_string();
    let uri = request.uri().to_string();
    let bytes = to_bytes(request.into_body(), 1024 * 1024).await.unwrap();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    state
        .lock()
        .unwrap()
        .requests
        .push(Seen { method, uri, body });
    let mut response = StatusCode::OK.into_response();
    response
        .headers_mut()
        .insert(header::LOCATION, "/upload/youtube/session".parse().unwrap());
    response
}

fn channel() -> Channel {
    Channel {
        identity: Identity {
            parent: None,
            id: "account-1".into(),
            name: "Fixture".into(),
            username: None,
        },
        tokens: Tokens {
            access_token: "fixture-only".into(),
            refresh_token: None,
            expires_at: None,
            scopes: vec![],
            scopes_known: false,
        },
        app: AppConfig {
            provider: "youtube".into(),
            client_id: "fixture-client".into(),
            client_secret: "fixture-secret".into(),
            instance_url: None,
            organization_access: false,
        },
    }
}

fn video_publication(options: Value) -> Publication {
    Publication {
        key: "fixture-publication".into(),
        text: "Fixture description".into(),
        title: "Fixture title".into(),
        media: vec![Media {
            name: "fixture.mp4".into(),
            mime: "video/mp4".into(),
            source: MediaSource::Bytes(support::fixture_bytes(true)),
            public_url: None,
            alt_text: String::new(),
        }],
        options,
    }
}

async fn fixture() -> (Adapter, Arc<Mutex<Fixture>>, tokio::task::JoinHandle<()>) {
    assert_eq!(
        std::env::var("BEWEFT_REQUIRE_PROVIDER_FIXTURES").as_deref(),
        Ok("true")
    );
    assert_eq!(
        std::env::var("BEWEFT_PROVIDER_TEST_SANDBOX").as_deref(),
        Ok("1")
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let adapter = Adapter::fixture(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
    let state = Arc::new(Mutex::new(Fixture::default()));
    let app = Router::new()
        .fallback(any(provider))
        .with_state(state.clone());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (adapter, state, task)
}

fn insert_request(state: &Arc<Mutex<Fixture>>) -> Seen {
    let requests = &state.lock().unwrap().requests;
    assert_eq!(
        requests.len(),
        1,
        "one resumable insert request is expected"
    );
    requests[0].clone()
}

#[tokio::test]
async fn youtube_options_round_trip_into_the_insert_request() {
    let (adapter, state, task) = fixture().await;
    let p = video_publication(json!({
        "privacy": "unlisted",
        "made_for_kids": false,
        "contains_synthetic_media": true,
        "category_id": "27",
        "license": "creativeCommon",
        "embeddable": false,
        "notify_subscribers": false,
    }));
    let Step::Checkpoint(cp) = adapter.publish_step(&channel(), &p, None).await.unwrap() else {
        panic!("session checkpoint must precede upload")
    };
    assert_eq!(cp["offset"], 0);

    let request = insert_request(&state);
    assert_eq!(request.method, "POST");
    assert_eq!(
        request.uri,
        "/upload/youtube/v3/videos?uploadType=resumable&part=snippet,status&notifySubscribers=false"
    );
    assert_eq!(request.body["snippet"]["title"], "Fixture title");
    assert_eq!(
        request.body["snippet"]["description"],
        "Fixture description"
    );
    assert_eq!(request.body["snippet"]["categoryId"], "27");
    assert_eq!(request.body["status"]["privacyStatus"], "unlisted");
    assert_eq!(request.body["status"]["selfDeclaredMadeForKids"], false);
    assert_eq!(request.body["status"]["containsSyntheticMedia"], true);
    assert_eq!(request.body["status"]["license"], "creativeCommon");
    assert_eq!(request.body["status"]["embeddable"], false);
    assert!(
        request.body["status"].get("publicStatsViewable").is_none(),
        "subscriber notification is an insert parameter, not a status field"
    );

    task.abort();
    let _ = task.await;
}

#[tokio::test]
async fn youtube_metadata_defaults_match_the_documented_api_defaults() {
    let (adapter, state, task) = fixture().await;
    let p = video_publication(json!({"privacy": "private", "made_for_kids": true}));
    adapter.publish_step(&channel(), &p, None).await.unwrap();

    let request = insert_request(&state);
    assert_eq!(request.body["status"]["license"], "youtube");
    assert_eq!(request.body["status"]["embeddable"], true);
    assert!(
        request.uri.ends_with("&notifySubscribers=true"),
        "default subscriber notification is sent explicitly"
    );

    task.abort();
    let _ = task.await;
}

#[tokio::test]
async fn youtube_invalid_metadata_is_rejected_before_any_request() {
    let (adapter, state, task) = fixture().await;
    let p = video_publication(json!({
        "privacy": "private",
        "made_for_kids": false,
        "license": "standard",
    }));
    assert!(adapter.publish_step(&channel(), &p, None).await.is_err());
    assert!(
        state.lock().unwrap().requests.is_empty(),
        "invalid metadata must not reach the network"
    );

    task.abort();
    let _ = task.await;
}

#[test]
fn youtube_metadata_rejects_values_outside_the_api_enums() {
    let mut p = video_publication(json!({"privacy": "private", "made_for_kids": false}));
    validate("youtube", &p).unwrap();

    for options in [
        json!({"privacy": "private", "made_for_kids": false, "license": "standard"}),
        json!({"privacy": "private", "made_for_kids": false, "license": 1}),
        json!({"privacy": "private", "made_for_kids": false, "embeddable": "no"}),
        json!({"privacy": "private", "made_for_kids": false, "embeddable": 1}),
        json!({"privacy": "private", "made_for_kids": false, "notify_subscribers": "true"}),
        json!({"privacy": "private", "made_for_kids": false, "notify_subscribers": 1}),
    ] {
        p.options = options;
        assert!(
            validate("youtube", &p).is_err(),
            "invalid YouTube metadata must be rejected at the boundary"
        );
    }

    p.options = json!({
        "privacy": "public",
        "made_for_kids": false,
        "license": "creativeCommon",
        "embeddable": true,
        "notify_subscribers": false,
    });
    validate("youtube", &p).unwrap();
}
