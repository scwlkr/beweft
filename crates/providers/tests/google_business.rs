//! Google API shapes over isolated loopback fixtures; never live provider calls.
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
    writes: usize,
    reads: usize,
    mode: String,
    body: Value,
}
async fn provider(State(state): State<Arc<Mutex<Fixture>>>, request: Request) -> Response {
    let path = request.uri().path().to_owned();
    let query = request.uri().query().unwrap_or("").to_owned();
    let method = request.method().to_string();
    let bytes = to_bytes(request.into_body(), 100_000).await.unwrap();
    let mut s = state.lock().unwrap();
    if path == "/v1/accounts" {
        if s.mode == "bad_page" && query.contains("pageToken=") {
            return StatusCode::BAD_GATEWAY.into_response();
        }
        if query.contains("pageToken=") {
            return Json(json!({"accounts":[{"name":"accounts/2","accountName":"Second"}]}))
                .into_response();
        }
        return Json(json!({"accounts":[{"name":"accounts/1","accountName":"First"}],"nextPageToken":"next-account"})).into_response();
    }
    if path == "/v1/accounts/1/locations" {
        assert!(query.contains("readMask=name%2Ctitle"));
        if query.contains("pageToken=") {
            if s.mode == "repeat" {
                return Json(json!({"locations":[],"nextPageToken":"repeat"})).into_response();
            }
            return Json(json!({"locations":[{"name":"locations/11","title":"Two"}]}))
                .into_response();
        }
        let next = if s.mode == "repeat" {
            "repeat"
        } else {
            "next-location"
        };
        return Json(
            json!({"locations":[{"name":"locations/10","title":"One"}],"nextPageToken":next}),
        )
        .into_response();
    }
    if path == "/v1/accounts/2/locations" {
        return Json(json!({"locations":[{"name":"locations/10","title":"One"}]})).into_response();
    }
    if path == "/v4/accounts/1/locations/10/localPosts" && method == "POST" {
        s.writes += 1;
        s.body = serde_json::from_slice(&bytes).unwrap();
        if s.mode == "lost" {
            return StatusCode::BAD_GATEWAY.into_response();
        }
        return Json(
            json!({"name":"accounts/1/locations/10/localPosts/post1","state":"PROCESSING"}),
        )
        .into_response();
    }
    if path == "/v4/accounts/1/locations/10/localPosts/post1" {
        s.reads += 1;
        if s.mode == "404" {
            return StatusCode::NOT_FOUND.into_response();
        }
        let status = match s.mode.as_str() {
            "live" => "LIVE",
            "recurring" => "RECURRING",
            "rejected" => "REJECTED",
            "unknown" => "NEW_STATE",
            _ => "PROCESSING",
        };
        return Json(json!({"name":"accounts/1/locations/10/localPosts/post1","state":status,"searchUrl":"https://example.invalid/post"})).into_response();
    }
    StatusCode::NOT_FOUND.into_response()
}
fn app() -> AppConfig {
    AppConfig {
        provider: "google_business".into(),
        client_id: "fixture".into(),
        client_secret: "fixture".into(),
        instance_url: None,
    }
}
fn tokens() -> Tokens {
    Tokens {
        access_token: "fixture".into(),
        refresh_token: None,
        expires_at: None,
        scopes: vec!["https://www.googleapis.com/auth/business.manage".into()],
        scopes_known: true,
    }
}
fn channel() -> Channel {
    Channel {
        app: app(),
        tokens: tokens(),
        identity: Identity {
            id: "locations/10".into(),
            parent: Some("accounts/1".into()),
            name: "One".into(),
            username: None,
        },
    }
}
fn publication() -> Publication {
    Publication {
        key: "fixture".into(),
        text: "Hello".into(),
        title: String::new(),
        media: vec![],
        options: json!({}),
    }
}
fn checkpoint(step: Step) -> Value {
    match step {
        Step::Checkpoint(v) => v,
        _ => panic!("expected checkpoint"),
    }
}

#[tokio::test]
async fn discovery_and_publication_have_safe_boundaries() {
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
            Router::new()
                .fallback(any(provider))
                .with_state(state.clone()),
        )
        .into_future(),
    );
    let adapter = Adapter::fixture(&origin).unwrap();
    let identities = adapter.identities(&app(), &tokens()).await.unwrap();
    assert_eq!(identities.len(), 3);
    assert_eq!(
        identities[0].selection_key().unwrap(),
        "accounts/1/locations/10"
    );
    assert_eq!(
        identities[2].selection_key().unwrap(),
        "accounts/2/locations/10"
    );
    assert_eq!(
        identities[0].destination_key(),
        identities[2].destination_key()
    );
    assert!(
        identities[0]
            .identity
            .username
            .as_ref()
            .unwrap()
            .contains("First")
    );
    state.lock().unwrap().mode = "bad_page".into();
    assert!(adapter.identities(&app(), &tokens()).await.is_err());
    state.lock().unwrap().mode = "repeat".into();
    assert!(adapter.identities(&app(), &tokens()).await.is_err());
    state.lock().unwrap().mode.clear();
    let auth = authorization_url(
        &app(),
        "https://example.invalid/callback",
        "state",
        "verifier",
    )
    .unwrap();
    assert!(
        auth.contains("business.manage")
            && auth.contains("code_challenge")
            && auth.contains("access_type=offline")
    );

    let c = channel();
    let p = publication();
    let start = checkpoint(adapter.publish_step(&c, &p, None).await.unwrap());
    assert_eq!(start["stage"], "google_post_started");
    assert_eq!(state.lock().unwrap().writes, 0);
    let processing = checkpoint(adapter.publish_step(&c, &p, Some(&start)).await.unwrap());
    assert_eq!(processing["stage"], "google_post_processing");
    assert_eq!(state.lock().unwrap().writes, 1);
    assert!(matches!(
        adapter
            .publish_step(&c, &p, Some(&processing))
            .await
            .unwrap(),
        Step::Wait(_)
    ));
    assert!(
        adapter
            .reconcile(&c, &p, &processing)
            .await
            .unwrap()
            .is_none()
    );
    for mode in ["404", "unknown"] {
        state.lock().unwrap().mode = mode.into();
        let result = adapter.reconcile(&c, &p, &processing).await;
        assert!(result.is_err() || matches!(result, Ok(None)));
    }
    for mode in ["live", "recurring"] {
        state.lock().unwrap().mode = mode.into();
        assert!(matches!(
            adapter.reconcile(&c, &p, &processing).await.unwrap(),
            Some(Step::Published { .. })
        ));
    }
    state.lock().unwrap().mode = "rejected".into();
    assert_eq!(
        checkpoint(
            adapter
                .reconcile(&c, &p, &processing)
                .await
                .unwrap()
                .unwrap()
        )["stage"],
        "google_post_rejected"
    );
    state.lock().unwrap().mode = "lost".into();
    assert_eq!(
        adapter
            .publish_step(&c, &p, Some(&start))
            .await
            .err()
            .unwrap()
            .kind,
        FailureKind::Uncertain
    );
    assert!(adapter.reconcile(&c, &p, &start).await.unwrap().is_none());
    assert_eq!(state.lock().unwrap().writes, 2);
    server.abort();
}

#[tokio::test]
async fn supported_options_are_mapped_and_unsupported_data_rejected() {
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
            Router::new()
                .fallback(any(provider))
                .with_state(state.clone()),
        )
        .into_future(),
    );
    let adapter = Adapter::fixture(&origin).unwrap();
    let c = channel();
    let mut p = publication();
    let mut bytes = support::fixture_bytes(false);
    bytes.resize(11000, 0);
    p.media = vec![Media {
        name: "photo.jpg".into(),
        mime: "image/jpeg".into(),
        source: MediaSource::Bytes(bytes),
        public_url: Some("https://fixture.google.com/expiring".into()),
        alt_text: "A saved description for other channels".into(),
    }];
    p.options = json!({"topic_type":"EVENT","language_code":"en-US","event":{"title":"Sale","start_date":"2026-11-03","start_time":"09:30","end_date":"2026-11-04","end_time":"17:00"},"call_to_action":{"type":"BOOK","url":"https://example.invalid/book"}});
    validate("google_business", &p).unwrap();
    let start = checkpoint(adapter.publish_step(&c, &p, None).await.unwrap());
    checkpoint(adapter.publish_step(&c, &p, Some(&start)).await.unwrap());
    let body = state.lock().unwrap().body.clone();
    assert_eq!(
        body["event"]["schedule"]["startDate"],
        json!({"year":2026,"month":11,"day":3})
    );
    assert_eq!(body["event"]["schedule"]["startTime"]["hours"], 9);
    assert_eq!(
        body["media"][0]["sourceUrl"],
        "https://fixture.google.com/expiring"
    );
    assert_eq!(body["media"][0]["mediaFormat"], "PHOTO");
    assert_eq!(body["media"][0].as_object().unwrap().len(), 2);
    assert!(body["media"][0].get("description").is_none());
    assert_eq!(
        p.media[0].alt_text,
        "A saved description for other channels"
    );
    assert_eq!(body["callToAction"]["actionType"], "BOOK");
    let event = p.options["event"].clone();
    p.options = json!({"topic_type":"OFFER","event":event,"offer":{"coupon_code":"SAVE","redeem_online_url":"https://example.invalid/redeem","terms_conditions":"Example"}});
    assert!(validate("google_business", &p).is_ok());
    p.options["call_to_action"] = json!({"type":"CALL"});
    assert!(validate("google_business", &p).is_err());
    p.options = json!({"topic_type":"PRODUCT"});
    assert!(validate("google_business", &p).is_err());
    p.options = json!({"topic_type":"EVENT","event":{"title":"Bad","start_date":"2026-02-30","start_time":"10:00","end_date":"2026-03-01","end_time":"12:00"}});
    assert!(validate("google_business", &p).is_err());
    p.options = json!({"topic_type":"EVENT","event":{"title":"Bad","start_date":"2026-03-01","start_time":"12:00","end_date":"2026-03-01","end_time":"10:00"}});
    assert!(validate("google_business", &p).is_err());
    p.options = json!({"unknown":true});
    assert!(validate("google_business", &p).is_err());
    p.options = json!({});
    assert!(validate("google_business", &p).is_ok());
    p.media[0].mime = "video/mp4".into();
    assert!(validate("google_business", &p).is_err());
    server.abort();
}
