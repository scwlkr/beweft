//! Isolated official-shape X fixtures; these are not live qualification.
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
    requests: Vec<(String, Value)>,
    chunks: Vec<(u64, usize)>,
    fail: String,
    status: String,
    polls: usize,
}

async fn provider(State(fixture): State<Arc<Mutex<Fixture>>>, request: Request) -> Response {
    let uri = request.uri().clone();
    let method = request.method().to_string();
    let path = uri.path().to_owned();
    let headers = request.headers().clone();
    let bytes = to_bytes(request.into_body(), 6 * 1024 * 1024)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    let mut fixture = fixture.lock().unwrap();
    fixture
        .requests
        .push((format!("{method} {path}"), body.clone()));
    if fixture.fail == path {
        return (StatusCode::BAD_GATEWAY, "lost fixture response").into_response();
    }
    match (method.as_str(), path.as_str()) {
        ("POST", "/2/media/upload/initialize") => {
            if fixture.status == "bad_init" { return Json(json!({"data":{"id":"not-numeric","expires_after_secs":60}})).into_response(); }
            Json(json!({"data":{"id":"123456789","expires_after_secs":86400}})).into_response()
        }
        ("POST", "/2/media/upload/123456789/append") => {
            let content_type = headers.get("content-type").unwrap().to_str().unwrap();
            let boundary = content_type.split("boundary=").nth(1).unwrap();
            let body = &bytes[..];
            let marker = b"name=\"segment_index\"\r\n\r\n";
            let start = body.windows(marker.len()).position(|w| w == marker).unwrap() + marker.len();
            let end = body[start..].windows(2).position(|w| w == b"\r\n").unwrap() + start;
            let index: u64 = std::str::from_utf8(&body[start..end]).unwrap().parse().unwrap();
            let media_marker = b"Content-Type: video/mp4\r\n\r\n";
            let media_start = body.windows(media_marker.len()).position(|w| w == media_marker).unwrap() + media_marker.len();
            let closing = format!("\r\n--{boundary}--\r\n");
            let media_end = body.windows(closing.len()).position(|w| w == closing.as_bytes()).unwrap();
            fixture.chunks.push((index, media_end - media_start));
            Json(json!({"data":{"expires_at":123}})).into_response()
        }
        ("POST", "/2/media/upload/123456789/finalize") => Json(json!({"data":{"id":"123456789","processing_info":{"state":"pending","check_after_secs":2}}})).into_response(),
        ("GET", "/2/media/upload") => {
            assert_eq!(uri.query(), Some("command=STATUS&media_id=123456789"));
            fixture.polls += 1;
            let status = if fixture.status == "failed" { "failed" } else if fixture.status == "invalid" { "mystery" } else if fixture.polls == 1 { "in_progress" } else { "succeeded" };
            Json(json!({"data":{"id":"123456789","processing_info":{"state":status,"check_after_secs":2}}})).into_response()
        }
        ("POST", "/2/tweets") => Json(json!({"data":{"id":"987654321"}})).into_response(),
        _ => (StatusCode::NOT_FOUND, "unexpected fixture route").into_response(),
    }
}

fn channel() -> Channel {
    Channel {
        identity: Identity {
            id: "1".into(),
            name: "Fixture".into(),
            username: None,
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
    let mut bytes = support::fixture_bytes(true);
    bytes.resize(9 * 1024 * 1024 + 7, 0);
    Publication {
        key: "fixture".into(),
        text: "fixture".into(),
        title: String::new(),
        options: json!({}),
        media: vec![Media {
            name: "fixture.mp4".into(),
            mime: "video/mp4".into(),
            source: MediaSource::Bytes(bytes),
            public_url: None,
            alt_text: String::new(),
        }],
    }
}

#[tokio::test]
async fn official_upload_shapes_restarts_and_unknown_writes() {
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
    let c = channel();
    let p = publication();
    let mut cp = None;
    let mut stages = vec![];
    for _ in 0..20 {
        // Reconstruct the adapter at every persisted stage, as after restart.
        let adapter = Adapter::fixture(&origin).unwrap();
        match adapter.publish_step(&c, &p, cp.as_ref()).await.unwrap() {
            Step::Checkpoint(next) => {
                stages.push(next["stage"].as_str().unwrap().to_owned());
                cp = Some(next);
            }
            Step::Wait(next) => {
                assert_eq!(next["check_after_secs"], 2);
                stages.push(next["stage"].as_str().unwrap().to_owned());
                cp = Some(next);
            }
            Step::Published { external_id, .. } => {
                assert_eq!(external_id, "987654321");
                break;
            }
        }
    }
    {
        let f = fixture.lock().unwrap();
        assert_eq!(
            f.chunks,
            vec![
                (0, 4 * 1024 * 1024),
                (1, 4 * 1024 * 1024),
                (2, 1024 * 1024 + 7)
            ]
        );
        let init = &f
            .requests
            .iter()
            .find(|(path, _)| path == "POST /2/media/upload/initialize")
            .unwrap()
            .1;
        assert_eq!(
            init,
            &json!({"media_type":"video/mp4","total_bytes":9*1024*1024+7,"media_category":"tweet_video"})
        );
        let post = &f
            .requests
            .iter()
            .find(|(path, _)| path == "POST /2/tweets")
            .unwrap()
            .1;
        assert_eq!(post["media"]["media_ids"], json!(["123456789"]));
        assert!(
            stages.contains(&"x_video_processing".to_owned())
                && stages.contains(&"x_post_started".to_owned())
        );
        assert_eq!(f.polls, 2);
    }

    let adapter = Adapter::fixture(&origin).unwrap();
    fixture.lock().unwrap().status = "bad_init".into();
    let Step::Checkpoint(initial) = adapter.publish_step(&c, &p, None).await.unwrap() else {
        panic!()
    };
    assert_eq!(
        adapter
            .publish_step(&c, &p, Some(&initial))
            .await
            .err()
            .unwrap()
            .kind,
        FailureKind::Uncertain
    );
    fixture.lock().unwrap().status.clear();
    let Step::Checkpoint(append) = adapter.publish_step(&c, &p, Some(&initial)).await.unwrap()
    else {
        panic!()
    };
    for (path, saved) in [
        ("/2/media/upload/123456789/append", append.clone()),
        (
            "/2/media/upload/123456789/finalize",
            json!({"stage":"x_video_finalize","id":"123456789","total_bytes":9*1024*1024+7,"offset":9*1024*1024+7}),
        ),
        (
            "/2/tweets",
            json!({"stage":"x_post_started","id":"123456789","total_bytes":9*1024*1024+7,"offset":9*1024*1024+7,"media_ids":["123456789"]}),
        ),
    ] {
        fixture.lock().unwrap().fail = path.into();
        assert_eq!(
            adapter
                .publish_step(&c, &p, Some(&saved))
                .await
                .err()
                .unwrap()
                .kind,
            FailureKind::Uncertain
        );
    }
    fixture.lock().unwrap().fail.clear();
    let processing = json!({"stage":"x_video_processing","id":"123456789","total_bytes":9*1024*1024+7,"offset":9*1024*1024+7});
    fixture.lock().unwrap().status = "failed".into();
    assert_eq!(
        adapter
            .publish_step(&c, &p, Some(&processing))
            .await
            .err()
            .unwrap()
            .kind,
        FailureKind::Rejected
    );
    fixture.lock().unwrap().status = "invalid".into();
    assert_eq!(
        adapter
            .publish_step(&c, &p, Some(&processing))
            .await
            .err()
            .unwrap()
            .kind,
        FailureKind::Uncertain
    );
    let before = fixture.lock().unwrap().requests.len();
    let invalid = json!({"stage":"x_video_append","id":"../bad","offset":0,"segment_index":0,"total_bytes":9*1024*1024+7});
    assert_eq!(
        adapter
            .publish_step(&c, &p, Some(&invalid))
            .await
            .err()
            .unwrap()
            .kind,
        FailureKind::Uncertain
    );
    assert_eq!(fixture.lock().unwrap().requests.len(), before);
    server.abort();
}
