use super::*;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;

/// Inspect only a forced media demuxer with no network protocols, no inherited secrets,
/// a bounded stdout reader, and a deadline. Playlists and external references are excluded.
async fn probe(m: &Media) -> Result<Value> {
    probe_with_program(m, "ffprobe").await
}

async fn probe_with_program(m: &Media, program: &str) -> Result<Value> {
    let format = match m.mime.as_str() {
        "image/jpeg" => "jpeg_pipe",
        "image/png" => "png_pipe",
        "image/webp" => "webp_pipe",
        "image/gif" => "gif",
        "video/mp4" | "video/quicktime" => "mov",
        "video/webm" => "matroska",
        _ => {
            return Err(invalid(
                "Unsupported media container; use JPEG, PNG, WebP, GIF, MP4, MOV or WebM",
            ));
        }
    };
    let mut cmd = tokio::process::Command::new(program);
    cmd.env_clear()
        .env("PATH", "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin")
        .kill_on_drop(true)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .args([
            "-v",
            "error",
            "-protocol_whitelist",
            "file,pipe",
            "-format_whitelist",
            format,
            "-f",
            format,
            "-probesize",
            "5000000",
            "-analyzeduration",
            "5000000",
            "-max_alloc",
            "33554432",
        ]);
    if format == "mov" {
        cmd.args(["-enable_drefs", "0", "-use_absolute_path", "0"]);
    }
    cmd.args([
        "-show_entries",
        "format=duration:stream=codec_type,codec_name,profile,width,height,duration,avg_frame_rate,sample_aspect_ratio,pix_fmt,field_order,channels",
        "-show_error",
        "-of",
        "json",
        "-i",
    ]);
    match &m.source {
        MediaSource::File { path, .. } => {
            cmd.arg(path);
        }
        MediaSource::Bytes(_) => {
            cmd.arg("pipe:0");
        }
    }
    let mut child = cmd.spawn().map_err(|_| {
        error(
            FailureKind::Unavailable,
            "Media inspection requires ffprobe installed on the server",
        )
    })?;
    let input = child.stdin.take();
    let output = child
        .stdout
        .take()
        .ok_or_else(|| error(FailureKind::Unavailable, "Media inspection failed"))?;
    let run = async {
        let write = async {
            if let MediaSource::Bytes(bytes) = &m.source {
                let mut stdin = input.ok_or_else(|| {
                    std::io::Error::other("Media inspection input is unavailable")
                })?;
                stdin.write_all(bytes).await?;
            }
            Ok::<_, std::io::Error>(())
        };
        let read = async {
            let mut bytes = vec![];
            output
                .take(65537)
                .read_to_end(&mut bytes)
                .await
                .map_err(|_| error(FailureKind::Unavailable, "Media inspection output failed"))?;
            if bytes.len() > 65536 {
                return Err(error(
                    FailureKind::Unavailable,
                    "Media inspection output exceeded its limit",
                ));
            }
            Ok::<_, ProviderError>(bytes)
        };
        let (written, bytes) = tokio::join!(write, read);
        let bytes = bytes?;
        let status = child
            .wait()
            .await
            .map_err(|_| error(FailureKind::Unavailable, "Media inspection failed"))?;
        let parsed: Value = serde_json::from_slice(&bytes).map_err(|_| {
            error(
                FailureKind::Unavailable,
                "Media inspection returned invalid output",
            )
        })?;
        if written
            .as_ref()
            .is_err_and(|error| error.kind() != std::io::ErrorKind::BrokenPipe)
        {
            return Err(error(
                FailureKind::Unavailable,
                "Media inspection input failed",
            ));
        }
        if !status.success() {
            // FFmpeg reports stable AVERROR_INVALIDDATA and AVERROR_EOF codes
            // for malformed content. OS errors, signals, missing structured
            // output and other failures remain retryable infrastructure errors.
            return Err(match parsed["error"]["code"].as_i64() {
                Some(-1094995529 | -541478725) => {
                    invalid("Media bytes are invalid or do not match their declared format")
                }
                _ => error(
                    FailureKind::Unavailable,
                    "Media inspection could not read the file",
                ),
            });
        }
        Ok(parsed)
    };
    tokio::time::timeout(Duration::from_secs(15), run)
        .await
        .map_err(|_| {
            error(
                FailureKind::Unavailable,
                "Media inspection exceeded its time limit",
            )
        })?
}

pub async fn inspect_media(provider: &str, p: &Publication) -> Result<()> {
    for m in &p.media {
        let bytes = m.source.len();
        let max = match (provider, m.mime.as_str()) {
            ("x", "image/gif") => 15 * 1024 * 1024,
            ("x", t) if t.starts_with("image/") => 5 * 1024 * 1024,
            ("x", _) => 256 * 1024 * 1024,
            ("bluesky", t) if t.starts_with("image/") => 2_000_000,
            ("bluesky", _) => 300_000_000,
            ("instagram" | "threads", t) if t.starts_with("image/") => 8 * 1024 * 1024,
            ("mastodon", t) if t.starts_with("image/") => 16 * 1024 * 1024,
            ("mastodon", _) => 99 * 1024 * 1024,
            ("facebook", t) if t.starts_with("image/") => 10 * 1024 * 1024,
            ("google_business", _) => 5 * 1024 * 1024,
            ("pinterest", _) => 20 * 1024 * 1024,
            _ => 256 * 1024 * 1024,
        };
        if provider == "google_business" && bytes < 10240 {
            return Err(invalid(
                "Google Business Profile photos must be at least 10,240 bytes",
            ));
        }
        if bytes == 0 || bytes > max {
            return Err(invalid(&format!(
                "Media exceeds the adapter's {max}-byte limit for this format"
            )));
        }
        let v = probe(m).await?;
        let streams = v["streams"]
            .as_array()
            .ok_or_else(|| invalid("Media has no readable streams"))?;
        let visuals: Vec<_> = streams
            .iter()
            .filter(|s| s["codec_type"] == "video")
            .collect();
        if visuals.len() != 1 || streams.len() > 4 {
            return Err(invalid(
                "Media must contain exactly one visual stream and at most four streams",
            ));
        }
        let visual = visuals[0];
        let width = visual["width"].as_u64().unwrap_or(0);
        let height = visual["height"].as_u64().unwrap_or(0);
        if provider == "google_business" && (width < 250 || height < 250) {
            return Err(invalid(
                "Google Business Profile photos must be at least 250 pixels on each side",
            ));
        }
        if width == 0
            || height == 0
            || width > 16384
            || height > 16384
            || width.saturating_mul(height) > 40_000_000
        {
            return Err(invalid(
                "Media dimensions are missing or exceed the supported 40-megapixel/16384-pixel limit",
            ));
        }
        let codec = visual["codec_name"].as_str().unwrap_or("");
        let expected = match m.mime.as_str() {
            "image/jpeg" => Some("mjpeg"),
            "image/png" => Some("png"),
            "image/webp" => Some("webp"),
            "image/gif" => Some("gif"),
            _ => None,
        };
        if expected.is_some_and(|e| e != codec) {
            return Err(invalid("Image bytes do not match the declared MIME type"));
        }
        if provider == "linkedin" && image(m) && width.saturating_mul(height) >= 36_152_320 {
            return Err(invalid(
                "LinkedIn images must have fewer than 36,152,320 pixels",
            ));
        }
        if provider == "threads" && image(m) {
            let ratio = width as f64 / height as f64;
            if !matches!(m.mime.as_str(), "image/jpeg" | "image/png")
                || !(320..=1440).contains(&width)
                || !(0.1..=10.0).contains(&ratio)
            {
                return Err(invalid(
                    "Threads photos require JPEG/PNG, width 320–1440 and aspect ratio between 1:10 and 10:1",
                ));
            }
        }
        if provider == "bluesky" && image(m) && (width > 4000 || height > 4000) {
            return Err(invalid(
                "Bluesky images must fit within 4000 by 4000 pixels",
            ));
        }
        if provider == "instagram" {
            let ratio = width as f64 / height as f64;
            if image(m) && (!(320..=1440).contains(&width) || !(0.8..=1.91).contains(&ratio)) {
                return Err(invalid(
                    "Instagram photos require width 320–1440 and aspect ratio 4:5–1.91:1",
                ));
            }
        }
        if video(m) {
            let duration = v["format"]["duration"]
                .as_str()
                .or_else(|| visual["duration"].as_str())
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);
            let max_duration = match provider {
                "instagram" => 900.0,
                "facebook" => 60.0,
                "threads" => 300.0,
                "mastodon" => 3600.0,
                "youtube" => 43200.0,
                "x" => 1200.0,
                "bluesky" => 600.0,
                _ => 900.0,
            };
            let minimum = match provider {
                "instagram" => 3.0,
                "facebook" => 4.0,
                "x" => 0.5,
                _ => 0.0,
            };
            if !duration.is_finite()
                || duration < minimum
                || duration <= 0.0
                || duration > max_duration
            {
                return Err(invalid(
                    "Video duration is missing or outside this provider format's supported range",
                ));
            }
            if matches!(provider, "instagram" | "threads")
                && (m.mime != "video/mp4"
                    || !matches!(codec, "h264" | "hevc")
                    || width > 1920
                    || streams
                        .iter()
                        .any(|s| s["codec_type"] == "audio" && s["codec_name"] != "aac"))
            {
                return Err(invalid(
                    "Meta video requires MP4, H.264/HEVC, optional AAC audio, and maximum width 1920",
                ));
            }
            if provider == "facebook" {
                let rate = visual["avg_frame_rate"].as_str().and_then(|s| {
                    let (n, d) = s.split_once('/')?;
                    let n: f64 = n.parse().ok()?;
                    let d: f64 = d.parse().ok()?;
                    (d > 0.0).then_some(n / d)
                });
                if m.mime != "video/mp4"
                    || !matches!(codec, "h264" | "hevc")
                    || width < 540
                    || height < 960
                    || width * 16 != height * 9
                    || !rate.is_some_and(|fps| fps.is_finite() && (23.0..=60.0).contains(&fps))
                    || streams
                        .iter()
                        .any(|s| s["codec_type"] == "audio" && s["codec_name"] != "aac")
                {
                    return Err(invalid(
                        "Facebook Reels require MP4 H.264/HEVC, optional AAC, at least 540×960 in 9:16, and 23–60 fps",
                    ));
                }
            }
            if provider == "x" {
                let rate = visual["avg_frame_rate"].as_str().and_then(|s| {
                    let (n, d) = s.split_once('/')?;
                    let n: f64 = n.parse().ok()?;
                    let d: f64 = d.parse().ok()?;
                    (d > 0.0).then_some(n / d)
                });
                let ratio = width as f64 / height as f64;
                let audio: Vec<_> = streams
                    .iter()
                    .filter(|s| s["codec_type"] == "audio")
                    .collect();
                if m.mime != "video/mp4"
                    || codec != "h264"
                    || width < 32
                    || height < 32
                    || width.max(height) > 1280
                    || width.min(height) > 1024
                    || !(1.0 / 3.0..=3.0).contains(&ratio)
                    || !rate.is_some_and(|fps| fps.is_finite() && fps > 0.0 && fps <= 60.0)
                    || visual["sample_aspect_ratio"] != "1:1"
                    || visual["pix_fmt"] != "yuv420p"
                    || visual["field_order"] != "progressive"
                    || audio.len() > 1
                    || audio.iter().any(|s| {
                        s["codec_name"] != "aac"
                            || s["profile"] != "LC"
                            || !matches!(s["channels"].as_u64(), Some(1 | 2))
                    })
                {
                    return Err(invalid(
                        "X video requires MP4/H.264, progressive YUV 4:2:0, square pixels, at most 60 fps and optional mono/stereo AAC-LC audio",
                    ));
                }
            }
        }
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    struct Scratch(std::path::PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[tokio::test]
    async fn structured_invalid_content_is_distinct_from_inspector_failures() {
        let dir = Scratch(
            std::env::temp_dir().join(format!("beweft-probe-test-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir(&dir.0).unwrap();
        let file = dir.0.join("fixture.jpg");
        std::fs::write(&file, b"fixture").unwrap();
        let media = Media {
            name: "fixture.jpg".into(),
            mime: "image/jpeg".into(),
            source: MediaSource::File {
                path: file,
                size: 7,
            },
            public_url: None,
            alt_text: String::new(),
        };
        let script = dir.0.join("inspector");
        for (output, expected) in [
            (r#"{"error":{"code":-1094995529}}"#, FailureKind::Rejected),
            (r#"{"error":{"code":-13}}"#, FailureKind::Unavailable),
            ("not-json", FailureKind::Unavailable),
        ] {
            std::fs::write(
                &script,
                format!("#!/bin/sh\nprintf '%s' '{output}'\nexit 1\n"),
            )
            .unwrap();
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
            let result = probe_with_program(&media, script.to_str().unwrap()).await;
            assert_eq!(result.unwrap_err().kind, expected);
        }
        let missing = dir.0.join("missing-inspector");
        assert_eq!(
            probe_with_program(&media, missing.to_str().unwrap())
                .await
                .unwrap_err()
                .kind,
            FailureKind::Unavailable
        );
    }
}
