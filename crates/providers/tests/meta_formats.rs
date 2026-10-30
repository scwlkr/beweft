//! Provider protocol fixtures: no live Meta access or public publication.
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
use std::{
    path::PathBuf,
    process::Command,
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct Fixture {
    requests: Vec<(String, Value)>,
    next: u64,
    statuses: usize,
    failure: String,
    rejection: String,
    processing: String,
    reel_publish_status: String,
    status_id: Option<String>,
    upload_bytes: usize,
}
async fn provider(State(state): State<Arc<Mutex<Fixture>>>, req: Request) -> Response {
    let path = req.uri().path().to_owned();
    let method = req.method().to_string();
    let headers = req.headers().clone();
    let bytes = to_bytes(req.into_body(), 16 * 1024 * 1024).await.unwrap();
    let body: Value = if headers
        .get("content-type")
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| h.starts_with("application/x-www-form-urlencoded"))
    {
        let map = url::form_urlencoded::parse(&bytes)
            .map(|(key, value)| {
                let value = match value.as_ref() {
                    "true" => json!(true),
                    "false" => json!(false),
                    _ => json!(value.as_ref()),
                };
                (key.into_owned(), value)
            })
            .collect::<serde_json::Map<String, Value>>();
        Value::Object(map)
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    let mut f = state.lock().unwrap();
    f.requests.push((format!("{method} {path}"), body.clone()));
    if f.failure == path {
        return (StatusCode::BAD_GATEWAY, "lost fixture response").into_response();
    }
    if f.rejection == path {
        return (StatusCode::BAD_REQUEST, "provider rejected fixture").into_response();
    }
    if path == "/v23.0/me/video_reels" {
        return if body["upload_phase"] == "start" {
            Json(json!({"video_id":"12345","upload_url":"https://rupload.facebook.com/video-upload/v23.0/12345"})).into_response()
        } else {
            assert_eq!(body["upload_phase"], "finish");
            assert_eq!(body["video_state"], "PUBLISHED");
            assert_eq!(body["video_id"], "12345");
            Json(json!({"success":true})).into_response()
        };
    }
    if path == "/v23.0/123/videos" {
        return Json(json!({"id":"12345"})).into_response();
    }
    if path == "/video-upload/v23.0/12345" {
        assert_eq!(headers["authorization"], "OAuth fixture-token");
        assert_eq!(headers["offset"], "0");
        assert_eq!(
            headers["file_size"]
                .to_str()
                .unwrap()
                .parse::<usize>()
                .unwrap(),
            bytes.len()
        );
        f.upload_bytes = bytes.len();
        return Json(json!({"success":true})).into_response();
    }
    if path == "/v23.0/12345" {
        f.statuses += 1;
        let phase = if f.statuses == 1 {
            "processing"
        } else {
            "complete"
        };
        let video_status = if f.processing == "REEL_ERROR" {
            "error"
        } else if phase == "complete" {
            "ready"
        } else {
            "processing"
        };
        let publish_status = if f.reel_publish_status.is_empty() {
            "published"
        } else {
            &f.reel_publish_status
        };
        return Json(json!({"id":"12345","status":{"video_status":video_status,"uploading_phase":{"status":"complete"},"processing_phase":{"status":"complete"},"publishing_phase":{"status":phase,"publish_status":publish_status}}})).into_response();
    }
    if path.ends_with("/media_publish") || path.ends_with("/threads_publish") {
        return Json(json!({"id":"999"})).into_response();
    }
    if path.ends_with("/media") || path.ends_with("/threads") {
        f.next += 1;
        return Json(json!({"id":f.next.to_string()})).into_response();
    }
    if method == "GET" && (path.starts_with("/v23.0/") || path.starts_with("/v1.0/")) {
        let status = if f.processing.is_empty() {
            "FINISHED"
        } else {
            &f.processing
        };
        let id = f
            .status_id
            .as_deref()
            .unwrap_or_else(|| path.rsplit('/').next().unwrap());
        return Json(json!({"id":id,"status_code":status,"status":status})).into_response();
    }
    (StatusCode::NOT_FOUND, "unexpected route").into_response()
}

#[tokio::test]
async fn facebook_video_and_instagram_stories_preserve_delivery_boundaries() {
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
    let adapter = Adapter::fixture(&origin).unwrap();

    // A landscape, ten-fps MP4 is accepted by the standard-video adapter;
    // the stricter Reel shape must not be silently applied to it.
    let fb = channel("facebook");
    let mut video = publication(vec![media(true)]);
    video.options = json!({"format":"video"});
    let Step::Checkpoint(start) = adapter.publish_step(&fb, &video, None).await.unwrap() else {
        panic!("public boundary")
    };
    assert_eq!(start["stage"], "fb_video_public");
    assert!(fixture.lock().unwrap().requests.is_empty());
    let Step::Checkpoint(processing) = adapter
        .publish_step(&fb, &video, Some(&start))
        .await
        .unwrap()
    else {
        panic!("video ID")
    };
    assert_eq!(processing["stage"], "fb_video_processing");
    assert_eq!(processing["video_id"], "12345");
    let Step::Wait(_) = adapter
        .publish_step(&fb, &video, Some(&processing))
        .await
        .unwrap()
    else {
        panic!("processing stays unresolved")
    };
    assert!(matches!(
        adapter
            .publish_step(&fb, &video, Some(&processing))
            .await
            .unwrap(),
        Step::Published { .. }
    ));
    {
        let f = fixture.lock().unwrap();
        let creates: Vec<_> = f
            .requests
            .iter()
            .filter(|(path, _)| path == "POST /v23.0/123/videos")
            .collect();
        assert_eq!(creates.len(), 1);
        assert_eq!(creates[0].1["file_url"], "https://media.fixture/video");
        assert_eq!(creates[0].1["description"], "Fixture caption");
        assert_eq!(creates[0].1["published"], true);
    }
    fixture.lock().unwrap().reel_publish_status = "scheduled".into();
    assert_eq!(
        adapter
            .reconcile(&fb, &video, &processing)
            .await
            .err()
            .unwrap()
            .kind,
        FailureKind::Uncertain
    );
    fixture.lock().unwrap().reel_publish_status.clear();
    fixture.lock().unwrap().rejection = "/v23.0/123/videos".into();
    assert_eq!(
        adapter
            .publish_step(&fb, &video, Some(&start))
            .await
            .err()
            .unwrap()
            .kind,
        FailureKind::Uncertain
    );
    fixture.lock().unwrap().rejection.clear();

    let ig = channel("instagram"); // Legacy grants have unknown scopes and no account-type metadata.
    let mut denied = ig.clone();
    denied.tokens.scopes_known = true;
    denied.tokens.scopes = vec!["instagram_business_basic".into()];
    let mut image = media(false);
    image.source = MediaSource::Bytes(support::custom_bytes(false, "540x960", "4"));
    let mut image_story = publication(vec![image]);
    image_story.options = json!({"format":"story"});
    assert_eq!(
        adapter
            .publish_step(&denied, &image_story, None)
            .await
            .err()
            .unwrap()
            .kind,
        FailureKind::Authentication
    );
    for is_video in [false, true] {
        let mut item = if is_video {
            reel()
        } else {
            image_story.media[0].clone()
        };
        item.public_url = Some(
            if is_video {
                "https://media.fixture/video"
            } else {
                "https://media.fixture/image"
            }
            .into(),
        );
        item.alt_text = "Keep this description locally".into();
        let mut story = publication(vec![item]);
        story.options = json!({"format":"story"});
        let start = fixture.lock().unwrap().requests.len();
        let Step::Checkpoint(container) = adapter.publish_step(&ig, &story, None).await.unwrap()
        else {
            panic!("container")
        };
        let Step::Checkpoint(ready) = adapter
            .publish_step(&ig, &story, Some(&container))
            .await
            .unwrap()
        else {
            panic!("ready")
        };
        let Step::Checkpoint(public) = adapter
            .publish_step(&ig, &story, Some(&ready))
            .await
            .unwrap()
        else {
            panic!("public boundary")
        };
        assert_eq!(public["stage"], "meta_public");
        assert!(matches!(
            adapter
                .publish_step(&ig, &story, Some(&public))
                .await
                .unwrap(),
            Step::Published { .. }
        ));
        let f = fixture.lock().unwrap();
        let requests = &f.requests[start..];
        let body = &requests
            .iter()
            .find(|(path, _)| path == "POST /v23.0/123/media")
            .unwrap()
            .1;
        assert_eq!(body["media_type"], "STORIES");
        assert!(body.get("caption").is_none() && body.get("alt_text").is_none());
        assert_eq!(
            body[if is_video { "video_url" } else { "image_url" }],
            if is_video {
                "https://media.fixture/video"
            } else {
                "https://media.fixture/image"
            }
        );
        assert_eq!(
            requests
                .iter()
                .filter(|(path, _)| path.ends_with("media_publish"))
                .count(),
            1
        );
    }
    let Step::Checkpoint(container) = adapter.publish_step(&ig, &image_story, None).await.unwrap()
    else {
        panic!("container")
    };
    fixture.lock().unwrap().status_id = Some("wrong".into());
    assert_eq!(
        adapter
            .publish_step(&ig, &image_story, Some(&container))
            .await
            .err()
            .unwrap()
            .kind,
        FailureKind::Uncertain
    );
    fixture.lock().unwrap().status_id = None;
    fixture.lock().unwrap().rejection = "/v23.0/123/media_publish".into();
    let public = json!({"stage":"meta_public","container_id":"42","expires_after_secs":3600});
    assert_eq!(
        adapter
            .publish_step(&ig, &image_story, Some(&public))
            .await
            .err()
            .unwrap()
            .kind,
        FailureKind::Uncertain
    );
    fixture.lock().unwrap().rejection.clear();
    fixture.lock().unwrap().processing = "PUBLISHED".into();
    let before = fixture.lock().unwrap().requests.len();
    assert!(
        adapter
            .reconcile(&ig, &image_story, &public)
            .await
            .unwrap()
            .is_none()
    );
    let f = fixture.lock().unwrap();
    let reads = &f.requests[before..];
    assert_eq!(reads.len(), 1);
    assert_eq!(reads[0].0, "GET /v23.0/42");
    drop(f);
    let mut unsupported = image_story.clone();
    unsupported.options["stickers"] = json!(["link"]);
    assert!(validate("instagram", &unsupported).is_err());
    server.abort();
    let _ = server.await;
}
fn channel(provider: &str) -> Channel {
    Channel {
        app: AppConfig {
            provider: provider.into(),
            client_id: "fixture".into(),
            client_secret: "fixture".into(),
            instance_url: None,
        },
        identity: Identity {
            parent: None,
            id: "123".into(),
            name: "Fixture".into(),
            username: None,
        },
        tokens: Tokens {
            access_token: "fixture-token".into(),
            refresh_token: None,
            expires_at: None,
            scopes: vec![],
            scopes_known: false,
        },
    }
}
fn media(video: bool) -> Media {
    Media {
        name: if video { "fixture.mp4" } else { "fixture.jpg" }.into(),
        mime: if video { "video/mp4" } else { "image/jpeg" }.into(),
        source: MediaSource::Bytes(support::fixture_bytes(video)),
        public_url: Some(
            if video {
                "https://media.fixture/video"
            } else {
                "https://media.fixture/image"
            }
            .into(),
        ),
        alt_text: if video { "" } else { "Image description" }.into(),
    }
}
fn publication(media: Vec<Media>) -> Publication {
    Publication {
        key: "fixture".into(),
        text: "Fixture caption".into(),
        title: "Fixture title".into(),
        options: json!({}),
        media,
    }
}
struct Remove(PathBuf);
impl Drop for Remove {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn reel() -> Media {
    let path = std::env::temp_dir().join(format!("beweft-reel-{}.mp4", uuid::Uuid::new_v4()));
    let _remove = Remove(path.clone());
    assert!(
        Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=blue:s=540x960:r=24",
                "-t",
                "4",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-pix_fmt",
                "yuv420p",
                "-movflags",
                "+faststart"
            ])
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    Media {
        name: "reel.mp4".into(),
        mime: "video/mp4".into(),
        source: MediaSource::Bytes(std::fs::read(&path).unwrap()),
        public_url: None,
        alt_text: String::new(),
    }
}

#[tokio::test]
async fn meta_carousels_and_reel_use_durable_official_shapes() {
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
    for name in ["instagram", "threads"] {
        let c = channel(name);
        let p = publication(vec![media(false), media(true)]);
        let mut cp = None;
        let mut stages = vec![];
        let start = fixture.lock().unwrap().requests.len();
        for _ in 0..20 {
            // Reconstruct on every saved stage to simulate a process restart.
            match Adapter::fixture(&origin)
                .unwrap()
                .publish_step(&c, &p, cp.as_ref())
                .await
                .unwrap()
            {
                Step::Checkpoint(next) | Step::Wait(next) => {
                    if next["stage"] == "meta_public" {
                        assert_eq!(next["children"].as_array().unwrap().len(), 2);
                    }
                    stages.push(next["stage"].as_str().unwrap().to_owned());
                    cp = Some(next);
                }
                Step::Published { external_id, .. } => {
                    assert_eq!(external_id, "999");
                    break;
                }
            }
        }
        assert_eq!(
            stages.iter().filter(|stage| *stage == "meta_child").count(),
            2
        );
        assert!(stages.contains(&"meta_public".into()));
        let f = fixture.lock().unwrap();
        let reqs = &f.requests[start..];
        let edge = if name == "instagram" {
            "/v23.0/123/media"
        } else {
            "/v1.0/123/threads"
        };
        let creates: Vec<_> = reqs
            .iter()
            .filter(|(path, _)| path == &format!("POST {edge}"))
            .collect();
        assert_eq!(creates.len(), 3);
        assert_eq!(creates[0].1["is_carousel_item"], true);
        assert_eq!(creates[0].1["image_url"], "https://media.fixture/image");
        assert_eq!(creates[0].1["alt_text"], "Image description");
        assert_eq!(creates[1].1["is_carousel_item"], true);
        assert_eq!(creates[1].1["media_type"], "VIDEO");
        assert_eq!(creates[1].1["video_url"], "https://media.fixture/video");
        assert_eq!(creates[2].1["media_type"], "CAROUSEL");
        let first_id = if name == "instagram" { 1 } else { 4 };
        assert_eq!(
            creates[2].1["children"],
            format!("{first_id},{}", first_id + 1)
        );
        assert_eq!(
            creates[2].1[if name == "instagram" {
                "caption"
            } else {
                "text"
            }],
            "Fixture caption"
        );
        assert_eq!(
            reqs.iter()
                .filter(|(path, _)| path.ends_with("_publish"))
                .count(),
            1
        );
    }
    let c = channel("facebook");
    let mut p = publication(vec![reel()]);
    p.options = json!({"format":"reel"});
    let mut cp = None;
    let mut stages = vec![];
    for _ in 0..12 {
        match Adapter::fixture(&origin)
            .unwrap()
            .publish_step(&c, &p, cp.as_ref())
            .await
            .unwrap()
        {
            Step::Checkpoint(next) | Step::Wait(next) => {
                if next["polls"].as_u64().is_some() {
                    assert_eq!(next["check_after_secs"], 60);
                }
                stages.push(next["stage"].as_str().unwrap().to_owned());
                cp = Some(next);
            }
            Step::Published {
                external_id,
                receipt,
                ..
            } => {
                assert_eq!(external_id, "12345");
                assert_eq!(receipt["status"], "publishing_complete");
                assert_eq!(receipt["publish_status"], "published");
                break;
            }
        }
    }
    assert!(
        stages.contains(&"fb_reel_public".into()) && stages.contains(&"fb_reel_processing".into())
    );
    assert!(fixture.lock().unwrap().upload_bytes > 0);
    assert_eq!(fixture.lock().unwrap().statuses, 2);

    let adapter = Adapter::fixture(&origin).unwrap();
    let mut missing_scope = c.clone();
    missing_scope.tokens.scopes_known = true;
    missing_scope.tokens.scopes = vec!["pages_manage_posts".into()];
    let before = fixture.lock().unwrap().requests.len();
    assert_eq!(
        adapter
            .publish_step(&missing_scope, &p, None)
            .await
            .err()
            .unwrap()
            .kind,
        FailureKind::Authentication
    );
    assert_eq!(fixture.lock().unwrap().requests.len(), before);
    let fb_public = json!({"stage":"fb_reel_public","video_id":"12345","expires_after_secs":3600});
    assert!(matches!(
        adapter.reconcile(&c, &p, &fb_public).await.unwrap(),
        Some(Step::Published { .. })
    ));
    fixture.lock().unwrap().reel_publish_status = "scheduled".into();
    assert_eq!(
        adapter
            .reconcile(&c, &p, &fb_public)
            .await
            .err()
            .unwrap()
            .kind,
        FailureKind::Uncertain
    );
    fixture.lock().unwrap().reel_publish_status.clear();
    fixture.lock().unwrap().processing = "REEL_ERROR".into();
    let fb_processing =
        json!({"stage":"fb_reel_processing","video_id":"12345","expires_after_secs":3600});
    assert_eq!(
        adapter
            .publish_step(&c, &p, Some(&fb_processing))
            .await
            .err()
            .unwrap()
            .kind,
        FailureKind::Uncertain
    );
    fixture.lock().unwrap().processing.clear();
    fixture.lock().unwrap().failure = "/video-upload/v23.0/12345".into();
    let before = fixture.lock().unwrap().requests.len();
    let fb_upload = json!({"stage":"fb_reel_upload","video_id":"12345","expires_after_secs":3600});
    assert_eq!(
        adapter
            .publish_step(&c, &p, Some(&fb_upload))
            .await
            .err()
            .unwrap()
            .kind,
        FailureKind::Uncertain
    );
    assert_eq!(fixture.lock().unwrap().requests.len(), before + 1);
    fixture.lock().unwrap().failure.clear();
    fixture.lock().unwrap().rejection = "/v23.0/123/media".into();
    let rejected_child = json!({"stage":"meta_build","children":[],"expires_after_secs":3600});
    let ig = channel("instagram");
    let images = publication(vec![media(false), media(false)]);
    assert_eq!(
        adapter
            .publish_step(&ig, &images, Some(&rejected_child))
            .await
            .err()
            .unwrap()
            .kind,
        FailureKind::Rejected
    );
    fixture.lock().unwrap().rejection.clear();
    fixture.lock().unwrap().failure = "/v23.0/123/media_publish".into();
    let before = fixture.lock().unwrap().requests.len();
    let failed = json!({"stage":"meta_public","container_id":"42","children":["40","41"],"expires_after_secs":3600});
    assert_eq!(
        adapter
            .publish_step(&ig, &images, Some(&failed))
            .await
            .err()
            .unwrap()
            .kind,
        FailureKind::Uncertain
    );
    assert_eq!(fixture.lock().unwrap().requests.len(), before + 1);
    fixture.lock().unwrap().failure.clear();
    fixture.lock().unwrap().processing = "IN_PROGRESS".into();
    let polling =
        json!({"stage":"meta_child","container_id":"42","children":[],"expires_after_secs":3600});
    let Step::Wait(wait) = adapter
        .publish_step(&ig, &images, Some(&polling))
        .await
        .unwrap()
    else {
        panic!("pending child should wait")
    };
    assert_eq!(wait["check_after_secs"], 60);
    let timed_out = json!({"stage":"meta_child","container_id":"42","children":[],"polls":4,"expires_after_secs":3600});
    assert_eq!(
        adapter
            .publish_step(&ig, &images, Some(&timed_out))
            .await
            .err()
            .unwrap()
            .kind,
        FailureKind::ExpiredSession
    );
    fixture.lock().unwrap().processing = "ERROR".into();
    let processing =
        json!({"stage":"meta_child","container_id":"42","children":[],"expires_after_secs":3600});
    assert_eq!(
        adapter
            .publish_step(&ig, &images, Some(&processing))
            .await
            .err()
            .unwrap()
            .kind,
        FailureKind::Rejected
    );
    let invalid = json!({"stage":"meta_build","children":["../bad"],"expires_after_secs":3600});
    let before = fixture.lock().unwrap().requests.len();
    assert_eq!(
        adapter
            .publish_step(&ig, &images, Some(&invalid))
            .await
            .err()
            .unwrap()
            .kind,
        FailureKind::Uncertain
    );
    assert_eq!(fixture.lock().unwrap().requests.len(), before);
    assert!(
        validate(
            "instagram",
            &Publication {
                options: json!({"format":"story"}),
                ..images.clone()
            }
        )
        .is_err()
    );
    assert!(
        validate(
            "facebook",
            &Publication {
                options: json!({}),
                ..p.clone()
            }
        )
        .is_err()
    );
    server.abort();
    let _ = server.await;
}
