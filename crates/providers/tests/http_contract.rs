//! Real HTTP integration against an isolated loopback provider fixture.
//! These assertions do not establish live network access or publication.
mod support;
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::any,
};
use beweft_providers::*;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
#[derive(Default)]
struct Fixture {
    requests: Vec<(String, Value)>,
    lost_write: bool,
    record: Value,
    session_origin: String,
}
async fn provider(State(state): State<Arc<Mutex<Fixture>>>, request: Request) -> Response {
    let path = request.uri().path().to_owned();
    let headers = request.headers().clone();
    let method = request.method().to_string();
    let bytes = to_bytes(request.into_body(), 20 * 1024 * 1024)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    let mut state = state.lock().unwrap();
    state
        .requests
        .push((format!("{method} {path}"), body.clone()));
    if state.lost_write && path == "/2/tweets" {
        return (StatusCode::BAD_GATEWAY, "fixture lost response").into_response();
    }
    let v = match path.as_str() {
        "/2/media/upload" => json!({"data":{"id":"123456789","expires_after_secs":86400}}),
        "/2/media/metadata" => json!({"data":{"id":body["id"]}}),
        "/2/tweets" => json!({"data":{"id":"987654321"}}),
        "/2/users/me" => json!({"data":{"id":"account-1","name":"Fixture","username":"fixture"}}),
        "/2/oauth2/token" => {
            json!({"access_token":"fixture-access","refresh_token":"fixture-refresh","expires_in":7200,"scope":"tweet.read tweet.write users.read offline.access media.write"})
        }
        "/upload/youtube/v3/videos" => {
            let mut r = StatusCode::OK.into_response();
            r.headers_mut().insert(
                header::LOCATION,
                format!("{}/upload/youtube/session", state.session_origin)
                    .parse()
                    .unwrap(),
            );
            return r;
        }
        "/upload/youtube/session" => {
            let range = headers.get("content-range").unwrap().to_str().unwrap();
            if range.starts_with("bytes */") || range.starts_with("bytes 0-8388607/") {
                let mut r = StatusCode::PERMANENT_REDIRECT.into_response();
                r.headers_mut()
                    .insert(header::RANGE, "bytes=0-8388607".parse().unwrap());
                return r;
            }
            json!({"id":"video-1","status":{"privacyStatus":"private","uploadStatus":"uploaded"}})
        }
        "/v23.0/account-1/feed" | "/v23.0/account-1/photos" => json!({"id":"facebook-post-1"}),
        "/v23.0/account-1/media" | "/v1.0/account-1/threads" => json!({"id":"container-1"}),
        "/v23.0/container-1" => json!({"status_code":"FINISHED"}),
        "/v1.0/container-1" => json!({"status":"FINISHED"}),
        "/v23.0/account-1/media_publish" | "/v1.0/account-1/threads_publish" => {
            json!({"id":"meta-post-1"})
        }
        "/api/v2/media" => json!({"id":"attachment-1","url":"https://fixture.invalid/media"}),
        "/api/v1/statuses" => {
            assert!(headers.contains_key("idempotency-key"));
            json!({"id":"status-1","url":"https://mastodon.example/@fixture/1"})
        }
        "/xrpc/com.atproto.repo.uploadBlob" => {
            json!({"blob":{"$type":"blob","ref":{"$link":"fixture"},"mimeType":"image/jpeg","size":4}})
        }
        "/xrpc/com.atproto.repo.createRecord" => {
            state.record = body["record"].clone();
            json!({"uri":format!("at://account-1/app.bsky.feed.post/{}",body["rkey"].as_str().unwrap()),"cid":"cid-1"})
        }
        "/xrpc/com.atproto.repo.getRecord" => {
            json!({"uri":"at://account-1/app.bsky.feed.post/fixed","cid":"cid-1","value":state.record})
        }
        "/rest/posts" => {
            let mut r = StatusCode::CREATED.into_response();
            r.headers_mut()
                .insert("x-restli-id", "urn:li:share:fixture".parse().unwrap());
            return r;
        }
        "/v5/pins" => json!({"id":"pin-1"}),
        _ => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error":"fixture route missing"})),
            )
                .into_response();
        }
    };
    Json(v).into_response()
}
fn channel(provider: &str) -> Channel {
    Channel {
        identity: Identity {
            id: "account-1".into(),
            name: "Fixture".into(),
            username: None,
        },
        tokens: Tokens {
            access_token: "fixture-only".into(),
            refresh_token: Some("fixture-refresh".into()),
            expires_at: None,
            scopes: vec![],
            scopes_known: false,
        },
        app: AppConfig {
            provider: provider.into(),
            client_id: "fixture-client".into(),
            client_secret: "fixture-secret".into(),
            instance_url: if matches!(provider, "mastodon" | "bluesky") {
                Some("https://mastodon.example".into())
            } else {
                None
            },
        },
    }
}
fn publication() -> Publication {
    Publication {
        key: "fixture-publication".into(),
        text: "Fixture content".into(),
        title: "Fixture title".into(),
        media: vec![],
        options: json!({}),
    }
}
fn picture() -> Media {
    Media {
        name: "fixture.jpg".into(),
        mime: "image/jpeg".into(),
        source: MediaSource::Bytes(support::fixture_bytes(false)),
        public_url: Some("https://media.example/fixture.jpg".into()),
        alt_text: "Fixture image".into(),
    }
}
#[tokio::test]
async fn direct_adapters_preserve_http_shapes_stages_and_ambiguous_outcomes() {
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
    let state = Arc::new(Mutex::new(Fixture {
        session_origin: origin.clone(),
        ..Default::default()
    }));
    let app = Router::new()
        .fallback(any(provider))
        .with_state(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let adapter = Adapter::fixture(&origin).unwrap();
    for provider_name in [
        "x",
        "facebook",
        "instagram",
        "threads",
        "mastodon",
        "bluesky",
        "linkedin",
        "pinterest",
    ] {
        let c = channel(provider_name);
        let mut p = publication();
        if provider_name != "linkedin" {
            p.media.push(picture());
        }
        if provider_name == "pinterest" {
            p.options = json!({"board_id":"fixture-board"});
        }
        let mut cp = None;
        let mut completed = false;
        for _ in 0..12 {
            match adapter.publish_step(&c, &p, cp.as_ref()).await.unwrap() {
                Step::Checkpoint(next) | Step::Wait(next) => cp = Some(next),
                Step::Published { external_id, .. } => {
                    assert!(!external_id.is_empty());
                    completed = true;
                    break;
                }
            }
        }
        assert!(completed, "fixture provider {provider_name} must complete");
        if provider_name == "bluesky" {
            assert!(matches!(
                adapter
                    .reconcile(&c, &p, cp.as_ref().unwrap())
                    .await
                    .unwrap(),
                Some(Step::Published { .. })
            ));
        }
    }
    let mut p = publication();
    p.media = vec![Media {
        name: "fixture.mp4".into(),
        mime: "video/mp4".into(),
        source: MediaSource::Bytes({
            let mut bytes = support::fixture_bytes(true);
            bytes.resize(9 * 1024 * 1024, 0);
            bytes
        }),
        public_url: None,
        alt_text: String::new(),
    }];
    p.options = json!({"privacy":"private","made_for_kids":false});
    let c = channel("youtube");
    let Step::Checkpoint(cp) = adapter.publish_step(&c, &p, None).await.unwrap() else {
        panic!("session checkpoint must precede upload")
    };
    assert_eq!(cp["offset"], 0);
    let Step::Checkpoint(next) = adapter.publish_step(&c, &p, Some(&cp)).await.unwrap() else {
        panic!("first chunk must persist progress")
    };
    assert_eq!(next["offset"], 8 * 1024 * 1024);
    let Some(Step::Checkpoint(reconciled)) = adapter.reconcile(&c, &p, &cp).await.unwrap() else {
        panic!("status probe must recover partial progress")
    };
    assert_eq!(reconciled["offset"], 8 * 1024 * 1024);
    assert!(matches!(
        adapter
            .publish_step(&c, &p, Some(&reconciled))
            .await
            .unwrap(),
        Step::Published { .. }
    ));
    let c = channel("x");
    let url = authorization_url(
        &c.app,
        "https://beweft.example/callback",
        "fixture-state",
        "fixture-verifier",
    )
    .unwrap();
    let parsed = url::Url::parse(&url).unwrap();
    assert!(
        parsed
            .query_pairs()
            .any(|(k, v)| k == "code_challenge_method" && v == "S256")
    );
    let tokens = adapter
        .exchange(
            &c.app,
            "fixture-code",
            "https://beweft.example/callback",
            "fixture-verifier",
        )
        .await
        .unwrap();
    assert_eq!(tokens.refresh_token.as_deref(), Some("fixture-refresh"));
    assert_eq!(
        adapter.identities(&c.app, &tokens).await.unwrap()[0]
            .identity
            .id,
        "account-1"
    );
    assert!(
        adapter
            .refresh(&c.app, &tokens)
            .await
            .unwrap()
            .expires_at
            .is_some()
    );
    state.lock().unwrap().lost_write = true;
    let before = state.lock().unwrap().requests.len();
    let Step::Checkpoint(started) = adapter
        .publish_step(&c, &publication(), None)
        .await
        .unwrap()
    else {
        panic!("the X public boundary must be checkpointed before a write")
    };
    let error = adapter
        .publish_step(&c, &publication(), Some(&started))
        .await
        .err()
        .unwrap();
    assert_eq!(error.kind, FailureKind::Uncertain);
    assert_eq!(
        state.lock().unwrap().requests.len(),
        before + 1,
        "ambiguous write must never be automatically duplicated"
    );
    assert!(!error.message.contains("fixture-only"));
    server.abort();
    let _ = server.await;
}
