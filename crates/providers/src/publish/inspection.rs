use super::*;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;

/// Inspect only a forced media demuxer with no network protocols, no inherited secrets,
/// a bounded stdout reader, and a deadline. Playlists and external references are excluded.
async fn probe(m: &Media) -> Result<Value> {
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
    let mut cmd = tokio::process::Command::new("ffprobe");
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
        "format=duration:stream=codec_type,codec_name,width,height,duration",
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
    let mut child = cmd
        .spawn()
        .map_err(|_| invalid("Media inspection requires ffprobe installed on the server"))?;
    let input = child.stdin.take();
    let output = child
        .stdout
        .take()
        .ok_or_else(|| invalid("Media inspection failed"))?;
    let run = async {
        let write = async {
            if let (Some(mut stdin), MediaSource::Bytes(bytes)) = (input, &m.source) {
                let _ = stdin.write_all(bytes).await;
            }
        };
        let read = async {
            let mut bytes = vec![];
            output
                .take(65537)
                .read_to_end(&mut bytes)
                .await
                .map_err(|_| invalid("Media inspection failed"))?;
            if bytes.len() > 65536 {
                return Err(invalid("Media has too many streams"));
            }
            Ok::<_, ProviderError>(bytes)
        };
        let (_, bytes) = tokio::join!(write, read);
        let bytes = bytes?;
        let status = child
            .wait()
            .await
            .map_err(|_| invalid("Media inspection failed"))?;
        if !status.success() {
            return Err(invalid(
                "Media bytes are invalid or do not match their declared format",
            ));
        }
        serde_json::from_slice(&bytes).map_err(|_| invalid("Media inspection failed"))
    };
    tokio::time::timeout(Duration::from_secs(15), run)
        .await
        .map_err(|_| invalid("Media inspection exceeded its time limit"))?
}

pub async fn inspect_media(provider: &str, p: &Publication) -> Result<()> {
    for m in &p.media {
        let bytes = m.source.len();
        let max = match (provider, m.mime.as_str()) {
            ("x", "image/gif") => 15 * 1024 * 1024,
            ("x", _) => 5 * 1024 * 1024,
            ("bluesky", _) => 2_000_000,
            ("instagram" | "threads", t) if t.starts_with("image/") => 8 * 1024 * 1024,
            ("mastodon", t) if t.starts_with("image/") => 16 * 1024 * 1024,
            ("mastodon", _) => 99 * 1024 * 1024,
            ("facebook", _) => 10 * 1024 * 1024,
            ("pinterest", _) => 20 * 1024 * 1024,
            _ => 256 * 1024 * 1024,
        };
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
        if provider == "bluesky" && (width > 4000 || height > 4000) {
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
                "threads" => 300.0,
                "mastodon" => 3600.0,
                "youtube" => 43200.0,
                _ => 900.0,
            };
            let minimum = if provider == "instagram" { 3.0 } else { 0.0 };
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
        }
    }
    Ok(())
}
