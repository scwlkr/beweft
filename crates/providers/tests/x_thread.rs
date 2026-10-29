//! Loopback-only X reply-chain fixtures; this does not qualify live access.
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
    posts: Vec<Value>,
    fail_at: usize,
    duplicate_at: usize,
}

async fn provider(State(fixture): State<Arc<Mutex<Fixture>>>, request: Request) -> Response {
    if request.uri().path() != "/2/tweets" {
        return StatusCode::NOT_FOUND.into_response();
    }
    let body: Value =
        serde_json::from_slice(&to_bytes(request.into_body(), 4096).await.unwrap()).unwrap();
    let mut fixture = fixture.lock().unwrap();
    fixture.posts.push(body);
    let index = fixture.posts.len();
    if index == fixture.fail_at {
        return StatusCode::BAD_GATEWAY.into_response();
    }
    let id = if index == fixture.duplicate_at {
        1001
    } else {
        1000 + index
    };
    Json(json!({"data":{"id":id.to_string()}})).into_response()
}

fn channel() -> Channel {
    Channel {
        identity: Identity {
            id: "1".into(),
            name: "Fixture".into(),
            username: None,
            parent: None,
        },
        tokens: Tokens {
            access_token: "fixture".into(),
            refresh_token: None,
            expires_at: None,
            scopes: vec![],
            scopes_known: false,
        },
        app: AppConfig {
            provider: "x".into(),
            client_id: "fixture".into(),
            client_secret: "fixture".into(),
            instance_url: None,
        },
    }
}

fn publication() -> Publication {
    Publication {
        key: "fixture".into(),
        text: "Root".into(),
        title: String::new(),
        media: vec![],
        options: json!({"x_thread":{"replies":["First reply","Second reply"]}}),
    }
}

#[tokio::test]
async fn ordered_checkpoints_full_receipt_and_invalid_thread_shapes() {
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
    let fixture = Arc::new(Mutex::new(Fixture::default()));
    let app = Router::new()
        .fallback(any(provider))
        .with_state(fixture.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let channel = channel();
    let publication = publication();
    let mut cp = None;
    let receipt = loop {
        let adapter = Adapter::fixture(&origin).unwrap();
        match adapter
            .publish_step(&channel, &publication, cp.as_ref())
            .await
            .unwrap()
        {
            Step::Checkpoint(next) => cp = Some(next),
            Step::Published {
                external_id,
                external_url,
                receipt,
            } => {
                assert_eq!(external_id, "1001");
                assert_eq!(external_url.as_deref(), Some("https://x.com/i/status/1001"));
                break receipt;
            }
            Step::Wait(_) => panic!("text thread should not wait"),
        }
    };
    assert_eq!(receipt["thread_ids"], json!(["1001", "1002", "1003"]));
    assert_eq!(
        fixture.lock().unwrap().posts,
        vec![
            json!({"text":"Root"}),
            json!({"text":"First reply","reply":{"in_reply_to_tweet_id":"1001"}}),
            json!({"text":"Second reply","reply":{"in_reply_to_tweet_id":"1002"}}),
        ]
    );
    assert_eq!(cp.unwrap()["stage"], "x_thread_complete");

    // An already-uploaded root attachment is sent only on the first post.
    {
        let mut f = fixture.lock().unwrap();
        f.posts.clear();
    }
    let mut with_media = publication.clone();
    with_media.media.push(Media {
        name: "root.jpg".into(),
        mime: "image/jpeg".into(),
        source: MediaSource::Bytes(support::fixture_bytes(false)),
        public_url: None,
        alt_text: String::new(),
    });
    let mut cp = Some(json!({"stage":"x_post_started","media_ids":["222"]}));
    loop {
        match Adapter::fixture(&origin)
            .unwrap()
            .publish_step(&channel, &with_media, cp.as_ref())
            .await
            .unwrap()
        {
            Step::Checkpoint(next) => cp = Some(next),
            Step::Published { .. } => break,
            Step::Wait(_) => panic!("image root should not wait"),
        }
    }
    let posts = fixture.lock().unwrap().posts.clone();
    assert_eq!(
        posts[0],
        json!({"text":"Root","media":{"media_ids":["222"]}})
    );
    assert!(posts[1].get("media").is_none() && posts[2].get("media").is_none());

    for duplicate in [false, true] {
        {
            let mut f = fixture.lock().unwrap();
            f.posts.clear();
            f.fail_at = if duplicate { 0 } else { 2 };
            f.duplicate_at = if duplicate { 2 } else { 0 };
        }
        let mut cp = None;
        loop {
            let adapter = Adapter::fixture(&origin).unwrap();
            match adapter
                .publish_step(&channel, &publication, cp.as_ref())
                .await
            {
                Ok(Step::Checkpoint(next)) => cp = Some(next),
                Err(error) => {
                    assert_eq!(error.kind, FailureKind::Uncertain);
                    let saved = cp.unwrap();
                    assert_eq!(saved["stage"], "x_thread_post_started");
                    assert_eq!(saved["confirmed_ids"], json!(["1001"]));
                    assert_eq!(saved["next_index"], 1);
                    break;
                }
                _ => panic!("the second request must remain unresolved"),
            }
        }
        assert_eq!(fixture.lock().unwrap().posts.len(), 2);
    }

    for options in [
        json!({"x_thread":{"replies":[]}}),
        json!({"x_thread":{"replies":[" "]}}),
        json!({"x_thread":{"replies":["ok"],"media_ids":["foreign"]}}),
        json!({"x_thread":{"replies":["x".repeat(281)]}}),
    ] {
        let mut bad = publication.clone();
        bad.options = options;
        assert!(validate("x", &bad).is_err());
        assert!(validate("facebook", &bad).is_err());
    }
    server.abort();
    let _ = server.await;
}
