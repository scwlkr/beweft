//! Official-shape Bluesky video fixtures. No live account or provider is contacted.
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
    mode: String,
    polls: usize,
    uploads: usize,
    posts: usize,
    record: Value,
    uri: String,
}

fn blob() -> Value {
    json!({"$type":"blob","ref":{"$link":"bafy-video-fixture"},"mimeType":"video/mp4","size":1024})
}

async fn provider(State(fixture): State<Arc<Mutex<Fixture>>>, request: Request) -> Response {
    let uri = request.uri().clone();
    let path = uri.path().to_owned();
    let method = request.method().to_string();
    let headers = request.headers().clone();
    let bytes = to_bytes(request.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    let query: std::collections::HashMap<_, _> =
        url::form_urlencoded::parse(uri.query().unwrap_or("").as_bytes())
            .into_owned()
            .collect();
    let mut f = fixture.lock().unwrap();
    match (method.as_str(), path.as_str()) {
        ("GET", "/xrpc/app.bsky.notification.listNotifications") => {
            assert_eq!(headers["authorization"], "Bearer fixture-access");
            assert_eq!(headers["atproto-proxy"], "did:web:api.bsky.app#bsky_notif");
            assert_eq!(query["limit"], "50");
            if f.mode == "invalid_notifications" {
                return Json(json!({})).into_response();
            }
            let notification = json!({"uri":"at://did:plc:other/app.bsky.feed.post/fixture", "reason":"mention", "author":{"handle":"other.bsky.social"}, "record":{"text":"Hello"}});
            Json(json!({"notifications":[notification, notification, {"uri":"at://ignored","reason":"follow"}]})).into_response()
        }
        ("GET", "/xrpc/com.atproto.server.getSession") => {
            assert_eq!(headers["authorization"], "Bearer fixture-access");
            let endpoint = if f.mode == "invalid_pds" {
                "http://127.0.0.1"
            } else {
                "https://assigned.pds.fixture"
            };
            Json(json!({"did":"did:plc:fixture","emailConfirmed":f.mode != "unconfirmed","didDoc":{"id":"did:plc:fixture","service":[{"id":"#atproto_pds","serviceEndpoint":endpoint}]}})).into_response()
        }
        ("GET", "/xrpc/com.atproto.server.getServiceAuth") => {
            assert_eq!(headers["authorization"], "Bearer fixture-access");
            assert_eq!(query["aud"], "did:web:assigned.pds.fixture");
            assert_eq!(query["lxm"], "com.atproto.repo.uploadBlob");
            let ttl = query["exp"].parse::<i64>().unwrap() - chrono::Utc::now().timestamp();
            assert!((1700..=1800).contains(&ttl));
            Json(json!({"token":"fixture-service-token"})).into_response()
        }
        ("POST", "/xrpc/app.bsky.video.uploadVideo") => {
            f.uploads += 1;
            assert_eq!(headers["authorization"], "Bearer fixture-service-token");
            assert_eq!(headers["content-type"], "video/mp4");
            assert_eq!(
                headers["content-length"]
                    .to_str()
                    .unwrap()
                    .parse::<usize>()
                    .unwrap(),
                bytes.len()
            );
            assert_eq!(query["did"], "did:plc:fixture");
            assert_eq!(query["name"], "video.mp4");
            if f.mode == "lost_upload" {
                return (StatusCode::BAD_GATEWAY, "lost").into_response();
            }
            if f.mode == "already_exists" {
                return (StatusCode::CONFLICT, Json(json!({"jobId":"job-1","did":"did:plc:fixture","state":"JOB_STATE_COMPLETED","error":"already_exists","blob":blob()}))).into_response();
            }
            Json(json!({"jobStatus":{"jobId":"job-1","did":"did:plc:fixture","state":"JOB_STATE_CREATED"}})).into_response()
        }
        ("GET", "/xrpc/app.bsky.video.getJobStatus") => {
            assert!(!headers.contains_key("authorization"));
            assert_eq!(query["jobId"], "job-1");
            f.polls += 1;
            let mut job =
                json!({"jobId":"job-1","did":"did:plc:fixture","state":"JOB_STATE_ENCODING"});
            if f.mode == "failed" {
                job["state"] = json!("JOB_STATE_FAILED");
                job["failureCode"] = json!("encoding_failure");
            } else if f.polls > 1 {
                job["state"] = json!("JOB_STATE_COMPLETED");
                job["blob"] = blob();
            }
            Json(json!({"jobStatus":job})).into_response()
        }
        ("POST", "/xrpc/com.atproto.repo.createRecord") => {
            f.posts += 1;
            f.record = body["record"].clone();
            f.uri = format!(
                "at://did:plc:fixture/app.bsky.feed.post/{}",
                body["rkey"].as_str().unwrap()
            );
            if f.mode == "lost_post" {
                return (StatusCode::BAD_GATEWAY, "lost").into_response();
            }
            Json(json!({"uri":f.uri,"cid":"cid-fixture"})).into_response()
        }
        ("GET", "/xrpc/com.atproto.repo.getRecord") => {
            if f.uri.is_empty() {
                return (StatusCode::NOT_FOUND, "missing").into_response();
            }
            let mut record = f.record.clone();
            if f.mode == "mismatch" {
                record["embed"]["alt"] = json!("different description");
            }
            Json(json!({"uri":f.uri,"cid":"cid-fixture","value":record})).into_response()
        }
        _ => (StatusCode::NOT_FOUND, "unexpected fixture route").into_response(),
    }
}

fn channel() -> Channel {
    Channel {
        identity: Identity {
            id: "did:plc:fixture".into(),
            name: "Fixture".into(),
            username: None,
        },
        tokens: Tokens {
            access_token: "fixture-access".into(),
            refresh_token: None,
            expires_at: None,
            scopes: vec![],
            scopes_known: false,
        },
        app: AppConfig {
            provider: "bluesky".into(),
            client_id: String::new(),
            client_secret: String::new(),
            instance_url: Some("https://pds.fixture".into()),
        },
    }
}
fn publication() -> Publication {
    Publication {
        key: "publication-fixture".into(),
        text: "Video fixture".into(),
        title: String::new(),
        options: json!({}),
        media: vec![Media {
            name: "fixture.mp4".into(),
            mime: "video/mp4".into(),
            source: MediaSource::Bytes(support::fixture_bytes(true)),
            public_url: None,
            alt_text: "A video description".into(),
        }],
    }
}

async fn step(origin: &str, c: &Channel, p: &Publication, cp: Option<&Value>) -> Step {
    Adapter::fixture(origin)
        .unwrap()
        .publish_step(c, p, cp)
        .await
        .unwrap()
}

#[tokio::test]
async fn video_receipts_survive_processing_and_reconcile_complete_records() {
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

    let adapter = Adapter::fixture(&origin).unwrap();
    let items = adapter.inbox(&c, &[]).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].text, "Hello");
    fixture.lock().unwrap().mode = "invalid_notifications".into();
    assert_eq!(
        adapter.inbox(&c, &[]).await.err().unwrap().kind,
        FailureKind::Unavailable
    );
    fixture.lock().unwrap().mode.clear();

    let Step::Checkpoint(prepared) = step(&origin, &c, &p, None).await else {
        panic!()
    };
    let Step::Checkpoint(preflight) = step(&origin, &c, &p, Some(&prepared)).await else {
        panic!()
    };
    fixture.lock().unwrap().mode = "unconfirmed".into();
    assert_eq!(
        Adapter::fixture(&origin)
            .unwrap()
            .publish_step(&c, &p, Some(&preflight))
            .await
            .err()
            .unwrap()
            .kind,
        FailureKind::Rejected
    );
    fixture.lock().unwrap().mode = "invalid_pds".into();
    assert_eq!(
        Adapter::fixture(&origin)
            .unwrap()
            .publish_step(&c, &p, Some(&preflight))
            .await
            .err()
            .unwrap()
            .kind,
        FailureKind::Rejected
    );
    assert_eq!(fixture.lock().unwrap().uploads, 0);
    fixture.lock().unwrap().mode.clear();
    let Step::Checkpoint(upload) = step(&origin, &c, &p, Some(&preflight)).await else {
        panic!()
    };
    assert_eq!(upload["stage"], "bsky_video_upload_started");
    let Step::Wait(job) = step(&origin, &c, &p, Some(&upload)).await else {
        panic!()
    };
    assert_eq!(job["job_id"], "job-1");
    assert!(!job.to_string().contains("fixture-service-token"));
    let Some(Step::Checkpoint(waiting)) = adapter.reconcile(&c, &p, &job).await.unwrap() else {
        panic!()
    };
    assert_eq!(waiting["job_id"], "job-1");
    let Step::Checkpoint(ready) = step(&origin, &c, &p, Some(&waiting)).await else {
        panic!()
    };
    assert_eq!(ready["blobs"][0], blob());
    let Step::Checkpoint(post) = step(&origin, &c, &p, Some(&ready)).await else {
        panic!()
    };
    fixture.lock().unwrap().mode = "lost_post".into();
    let err = Adapter::fixture(&origin)
        .unwrap()
        .publish_step(&c, &p, Some(&post))
        .await
        .err()
        .unwrap();
    assert_eq!(err.kind, FailureKind::Uncertain);
    assert!(matches!(
        adapter.reconcile(&c, &p, &post).await.unwrap(),
        Some(Step::Published { .. })
    ));
    assert_eq!(fixture.lock().unwrap().posts, 1);
    assert_eq!(
        fixture.lock().unwrap().record["embed"]["alt"],
        "A video description"
    );
    fixture.lock().unwrap().mode = "mismatch".into();
    assert_eq!(
        adapter.reconcile(&c, &p, &post).await.err().unwrap().kind,
        FailureKind::Uncertain
    );
    fixture.lock().unwrap().uri.clear();
    assert!(adapter.reconcile(&c, &p, &post).await.is_err());
    assert_eq!(fixture.lock().unwrap().posts, 1);

    fixture.lock().unwrap().mode = "lost_upload".into();
    let err = adapter
        .publish_step(&c, &p, Some(&upload))
        .await
        .err()
        .unwrap();
    assert_eq!(err.kind, FailureKind::Uncertain);
    assert!(!err.message.contains("fixture-service-token"));
    fixture.lock().unwrap().mode = "failed".into();
    let Step::Wait(failed_job) = step(&origin, &c, &p, Some(&upload)).await else {
        panic!()
    };
    assert_eq!(
        adapter
            .publish_step(&c, &p, Some(&failed_job))
            .await
            .err()
            .unwrap()
            .kind,
        FailureKind::Rejected
    );
    assert_eq!(fixture.lock().unwrap().posts, 1);
    fixture.lock().unwrap().mode = "already_exists".into();
    let Step::Checkpoint(existing) = step(&origin, &c, &p, Some(&upload)).await else {
        panic!()
    };
    assert_eq!(existing["blobs"][0], blob());
    server.abort();
    let _ = server.await;
}
