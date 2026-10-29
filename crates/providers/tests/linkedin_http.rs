//! Official-shape LinkedIn fixtures; no live accounts or provider calls.
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
    uploads: usize,
    polls: usize,
    fail: String,
    post: Value,
}
async fn provider(State(state): State<Arc<Mutex<Fixture>>>, request: Request) -> Response {
    let path = request.uri().path().to_owned();
    let query = request.uri().query().unwrap_or("").to_owned();
    let method = request.method().to_string();
    let headers = request.headers().clone();
    let bytes = to_bytes(request.into_body(), 6 * 1024 * 1024)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    let mut s = state.lock().unwrap();
    s.requests
        .push((format!("{method} {path}?{query}"), body.clone()));
    let expiry = chrono::Utc::now().timestamp_millis() + 60_000;
    if path == "/rest/images" && query == "action=initializeUpload" {
        assert_eq!(headers["linkedin-version"], "202610");
        assert_eq!(
            body["initializeUploadRequest"]["owner"],
            "urn:li:person:member"
        );
        s.uploads += 1;
        let upload = if s.fail == "bad_url" {
            "https://untrusted.example/dms-uploads/image".to_string()
        } else {
            format!("https://www.linkedin.com/dms-uploads/image{}", s.uploads)
        };
        return Json(json!({"value":{"image":format!("urn:li:image:image{}",s.uploads),"uploadUrl":upload,"uploadUrlExpiresAt":expiry}})).into_response();
    }
    if path.starts_with("/dms-uploads/image") {
        assert_eq!(method, "PUT");
        assert_eq!(headers["authorization"], "Bearer fixture-token");
        assert!(!bytes.is_empty());
        if s.fail == "upload" {
            return StatusCode::BAD_GATEWAY.into_response();
        };
        return StatusCode::CREATED.into_response();
    }
    if path.starts_with("/rest/images/") {
        assert!(headers.get("linkedin-version").is_none());
        s.polls += 1;
        let id = if path.ends_with("image1") {
            "image1"
        } else {
            "image2"
        };
        let status = if s.fail == "processing" {
            "PROCESSING_FAILED"
        } else if s.polls == 1 {
            "PROCESSING"
        } else {
            "AVAILABLE"
        };
        return Json(json!({"id":format!("urn:li:image:{id}"),"owner":"urn:li:person:member","status":status})).into_response();
    }
    if path == "/rest/videos" && query == "action=initializeUpload" {
        let size = body["initializeUploadRequest"]["fileSizeBytes"]
            .as_u64()
            .unwrap();
        let parts:Vec<Value>=(0..size.div_ceil(4*1024*1024)).map(|i|json!({"firstByte":i*4*1024*1024,"lastByte":((i+1)*4*1024*1024-1).min(size-1),"uploadUrl":format!("https://www.linkedin.com/dms-uploads/video{i}")})).collect();
        return Json(json!({"value":{"video":"urn:li:video:video1","uploadUrlsExpireAt":expiry,"uploadInstructions":parts,"uploadToken":""}})).into_response();
    }
    if path.starts_with("/dms-uploads/video") {
        assert_eq!(method, "PUT");
        assert!(headers.get("authorization").is_none());
        assert!(!bytes.is_empty());
        let mut r = StatusCode::OK.into_response();
        r.headers_mut().insert(
            header::ETAG,
            format!("\"part{}\"", path.trim_start_matches("/dms-uploads/video"))
                .parse()
                .unwrap(),
        );
        return r;
    }
    if path == "/rest/videos" && query == "action=finalizeUpload" {
        assert_eq!(
            body["finalizeUploadRequest"]["uploadedPartIds"],
            json!(["part0", "part1"])
        );
        return StatusCode::OK.into_response();
    }
    if path.starts_with("/rest/videos/") {
        s.polls += 1;
        let status = if s.polls == 1 {
            "PROCESSING"
        } else {
            "AVAILABLE"
        };
        return Json(
            json!({"id":"urn:li:video:video1","owner":"urn:li:person:member","status":status}),
        )
        .into_response();
    }
    if path == "/rest/posts" {
        s.post = body;
        if s.fail == "post" {
            return StatusCode::BAD_GATEWAY.into_response();
        };
        let mut r = StatusCode::CREATED.into_response();
        r.headers_mut()
            .insert("x-restli-id", "urn:li:share:12345".parse().unwrap());
        return r;
    }
    StatusCode::NOT_FOUND.into_response()
}
fn channel() -> Channel {
    Channel {
        identity: Identity {
            parent: None,
            id: "member".into(),
            name: "Member".into(),
            username: None,
        },
        tokens: Tokens {
            access_token: "fixture-token".into(),
            refresh_token: None,
            expires_at: None,
            scopes: vec!["w_member_social".into()],
            scopes_known: true,
        },
        app: AppConfig {
            provider: "linkedin".into(),
            client_id: "fixture".into(),
            client_secret: "fixture".into(),
            instance_url: None,
        },
    }
}
fn publication(video: bool) -> Publication {
    let mut bytes = support::fixture_bytes(video);
    if video {
        bytes.resize(4 * 1024 * 1024 + 10, 0)
    }
    let media = Media {
        name: if video { "video.mp4" } else { "image.jpg" }.into(),
        mime: if video { "video/mp4" } else { "image/jpeg" }.into(),
        source: MediaSource::Bytes(bytes),
        public_url: None,
        alt_text: "A blue scene".into(),
    };
    Publication {
        key: "fixture".into(),
        text: "Example".into(),
        title: "Video title".into(),
        media: if video {
            vec![media]
        } else {
            vec![media.clone(), media]
        },
        options: json!({}),
    }
}
#[tokio::test]
async fn linkedin_images_and_video_checkpoint_through_processing_and_one_post() {
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
    let app = Router::new()
        .fallback(any(provider))
        .with_state(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let adapter = Adapter::fixture(&origin).unwrap();
    for is_video in [false, true] {
        *state.lock().unwrap() = Fixture::default();
        let p = publication(is_video);
        let c = channel();
        let mut cp = None;
        let mut waited = false;
        let mut completed = false;
        for _ in 0..25 {
            match adapter.publish_step(&c, &p, cp.as_ref()).await.unwrap() {
                Step::Checkpoint(next) => cp = Some(next),
                Step::Wait(next) => {
                    waited = true;
                    cp = Some(next)
                }
                Step::Published { external_id, .. } => {
                    assert_eq!(external_id, "urn:li:share:12345");
                    completed = true;
                    break;
                }
            }
        }
        assert!(waited && completed);
        let s = state.lock().unwrap();
        assert_eq!(
            s.requests
                .iter()
                .filter(|(path, _)| path.starts_with("POST /rest/posts"))
                .count(),
            1
        );
        if is_video {
            assert_eq!(s.post["content"]["media"]["id"], "urn:li:video:video1");
            assert_eq!(s.post["content"]["media"]["title"], "Video title");
            assert_eq!(s.post["content"]["media"]["altText"], "A blue scene");
        } else {
            assert_eq!(
                s.post["content"]["multiImage"]["images"]
                    .as_array()
                    .unwrap()
                    .len(),
                2
            );
            assert_eq!(
                s.post["content"]["multiImage"]["images"][0]["altText"],
                "A blue scene"
            );
        }
    }
    *state.lock().unwrap() = Fixture {
        fail: "processing".into(),
        ..Default::default()
    };
    let p = publication(false);
    let c = channel();
    let mut cp = None;
    for _ in 0..8 {
        match adapter.publish_step(&c, &p, cp.as_ref()).await {
            Ok(Step::Checkpoint(next) | Step::Wait(next)) => cp = Some(next),
            Err(e) => {
                assert_eq!(e.kind, FailureKind::Rejected);
                break;
            }
            _ => panic!("failed processing cannot publish"),
        }
    }
    assert!(
        !state
            .lock()
            .unwrap()
            .requests
            .iter()
            .any(|(path, _)| path.starts_with("POST /rest/posts"))
    );
    // A provider-supplied upload URL cannot redirect the member bearer token.
    *state.lock().unwrap() = Fixture {
        fail: "bad_url".into(),
        ..Default::default()
    };
    let Step::Checkpoint(init) = adapter.publish_step(&c, &p, None).await.unwrap() else {
        panic!("expected image initialization")
    };
    let error = match adapter.publish_step(&c, &p, Some(&init)).await {
        Err(error) => error,
        Ok(_) => panic!("untrusted upload URL must be rejected"),
    };
    assert_eq!(error.kind, FailureKind::Uncertain);
    assert_eq!(state.lock().unwrap().requests.len(), 1);

    // A provider that never finishes cannot be polled indefinitely.
    *state.lock().unwrap() = Fixture::default();
    let mut cp = None;
    for _ in 0..3 {
        let Step::Checkpoint(next) = adapter.publish_step(&c, &p, cp.as_ref()).await.unwrap()
        else {
            panic!("image upload must advance through private checkpoints")
        };
        cp = Some(next);
    }
    let mut cp = cp.unwrap();
    assert_eq!(cp["stage"], "linkedin_image_processing");
    cp["poll_count"] = json!(1200);
    let error = match adapter.publish_step(&c, &p, Some(&cp)).await {
        Err(error) => error,
        Ok(_) => panic!("exhausted processing must be quarantined"),
    };
    assert_eq!(error.kind, FailureKind::Uncertain);
    assert!(
        !state
            .lock()
            .unwrap()
            .requests
            .iter()
            .any(|(path, _)| path.starts_with("POST /rest/posts"))
    );
    server.abort();
}
