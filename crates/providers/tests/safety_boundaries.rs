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
struct Seen(Vec<Value>);
async fn handler(State(seen): State<Arc<Mutex<Seen>>>, req: Request) -> Response {
    let path = req.uri().path().to_string();
    if path.contains("session") {
        return StatusCode::NOT_FOUND.into_response();
    }
    if path.contains("notifications") {
        return Json(json!([{"account":{"acct":"author@remote.example"},"status":{"id":"private-status","visibility":"direct","account":{"id":"author","acct":"author@remote.example"},"mentions":[{"id":"self","acct":"self@home.example"},{"id":"friend","acct":"friend@remote.example"}],"content":"private text"}}])).into_response();
    }
    let bytes = to_bytes(req.into_body(), 1_000_000).await.unwrap();
    seen.lock()
        .unwrap()
        .0
        .push(serde_json::from_slice(&bytes).unwrap_or(Value::Null));
    Json(json!({"id":"reply"})).into_response()
}
fn channel(provider: &str) -> Channel {
    Channel {
        app: AppConfig {
            provider: provider.into(),
            client_id: "client".into(),
            client_secret: "secret".into(),
            instance_url: Some("https://home.example".into()),
        },
        identity: Identity {
            id: "self".into(),
            name: "Self".into(),
            username: None,
        },
        tokens: Tokens {
            access_token: "fixture".into(),
            refresh_token: None,
            expires_at: None,
            scopes: vec![],
            scopes_known: false,
        },
    }
}
async fn fixture() -> (Adapter, Arc<Mutex<Seen>>, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let adapter = Adapter::fixture(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
    let seen = Arc::new(Mutex::new(Seen::default()));
    let app = Router::new()
        .fallback(any(handler))
        .with_state(seen.clone());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (adapter, seen, task)
}
#[tokio::test]
async fn mastodon_direct_and_followers_replies_preserve_audience() {
    let (adapter, seen, task) = fixture().await;
    let c = channel("mastodon");
    let mut item = adapter.inbox(&c, &[]).await.unwrap().remove(0);
    assert_eq!(item.reply_context.as_ref().unwrap().recipients.len(), 2);
    for visibility in ["direct", "private"] {
        item.reply_context.as_mut().unwrap().visibility = visibility.into();
        adapter
            .reply(&c, &item, "Acknowledged", "unique")
            .await
            .unwrap();
        let body = seen.lock().unwrap().0.last().unwrap().clone();
        assert_eq!(body["visibility"], "direct");
        assert_eq!(body["allowed_mentions"], json!(["author", "friend"]));
        assert_eq!(
            body["status"],
            "@author@remote.example @friend@remote.example Acknowledged"
        );
    }
    assert!(
        adapter
            .reply(&c, &item, "@outsider secret", "new")
            .await
            .is_err()
    );
    item.reply_context = None;
    assert!(adapter.reply(&c, &item, "secret", "new").await.is_err());
    assert_eq!(
        seen.lock().unwrap().0.len(),
        2,
        "rejected audience must cause no write"
    );
    task.abort();
    let _ = task.await;
}
#[tokio::test]
async fn expired_youtube_probe_never_creates_a_replacement_upload() {
    let (adapter, seen, task) = fixture().await;
    let p = Publication {
        key: "k".into(),
        text: "t".into(),
        title: "Title".into(),
        media: vec![Media {
            name: "v.mp4".into(),
            mime: "video/mp4".into(),
            source: MediaSource::Bytes(vec![1]),
            public_url: None,
            alt_text: String::new(),
        }],
        options: json!({}),
    };
    let result = adapter
        .reconcile(
            &channel("youtube"),
            &p,
            &json!({"session":"https://www.googleapis.com/session"}),
        )
        .await;
    assert_eq!(result.err().unwrap().kind, FailureKind::ExpiredSession);
    assert!(seen.lock().unwrap().0.is_empty());
    task.abort();
    let _ = task.await;
}
#[tokio::test]
async fn malformed_oversized_and_incompatible_media_cause_no_provider_write() {
    let (adapter, seen, task) = fixture().await;
    let c = channel("x");
    let m = Media {
        name: "bad.jpg".into(),
        mime: "image/jpeg".into(),
        source: MediaSource::Bytes(b"not an image".to_vec()),
        public_url: None,
        alt_text: String::new(),
    };
    let mut p = Publication {
        key: "k".into(),
        text: "t".into(),
        title: "Title".into(),
        media: vec![m.clone()],
        options: json!({}),
    };
    assert!(adapter.publish_step(&c, &p, None).await.is_err());
    p.media[0].source = MediaSource::Bytes(vec![0; 6 * 1024 * 1024]);
    assert!(adapter.publish_step(&c, &p, None).await.is_err());
    p.media[0].mime = "image/gif".into();
    p.media.push(m);
    assert!(adapter.publish_step(&c, &p, None).await.is_err());
    p.media.truncate(1);
    p.media[0].mime = "video/mp4".into();
    p.media[0].source = MediaSource::Bytes(support::fixture_bytes(true));
    p.options = json!({"privacy":"private","made_for_kids":false});
    inspect_media("youtube", &p).await.unwrap();
    p.media[0].source = MediaSource::Bytes(support::custom_bytes(true, "640x480", "1"));
    let short = adapter
        .publish_step(&channel("instagram"), &p, None)
        .await
        .err()
        .unwrap();
    assert!(short.message.contains("duration"));
    p.media[0].mime = "image/jpeg".into();
    p.media[0].source = MediaSource::Bytes(support::custom_bytes(false, "32x32", "1"));
    let small = adapter
        .publish_step(&channel("instagram"), &p, None)
        .await
        .err()
        .unwrap();
    assert!(small.message.contains("width"));

    assert!(seen.lock().unwrap().0.is_empty());
    task.abort();
    let _ = task.await;
}
#[tokio::test]
async fn permissions_distinguish_unknown_missing_and_granted_without_writes() {
    let (adapter, seen, task) = fixture().await;
    let mut c = channel("x");
    assert_eq!(
        grant_permissions(&c)["actions"]["publish"]["status"],
        "unknown"
    );
    c.tokens.scopes_known = true;
    c.tokens.scopes = vec!["tweet.read".into()];
    let p = Publication {
        key: "k".into(),
        text: "t".into(),
        title: "".into(),
        media: vec![],
        options: json!({}),
    };
    assert_eq!(
        adapter.publish_step(&c, &p, None).await.err().unwrap().kind,
        FailureKind::Authentication
    );
    c.tokens.scopes.push("tweet.write".into());
    assert_eq!(
        grant_permissions(&c)["actions"]["publish"]["status"],
        "granted"
    );
    assert!(seen.lock().unwrap().0.is_empty());
    task.abort();
    let _ = task.await;
}
