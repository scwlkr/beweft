use crate::*;
use reqwest::multipart::{Form, Part};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;
pub const BLUESKY_VIDEO_RATE_LIMIT: &str = "Bluesky video limit reached. Wait before retrying.";
#[derive(Clone)]
pub enum MediaSource {
    Bytes(Vec<u8>),
    File { path: std::path::PathBuf, size: u64 },
}
impl MediaSource {
    pub fn len(&self) -> u64 {
        match self {
            Self::Bytes(b) => b.len() as u64,
            Self::File { size, .. } => *size,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    async fn body(&self, offset: u64, length: u64) -> Result<reqwest::Body> {
        match self {
            Self::Bytes(b) => Ok(reqwest::Body::from(
                b[offset as usize..(offset + length) as usize].to_vec(),
            )),
            Self::File { path, .. } => {
                let mut file = tokio::fs::File::open(path)
                    .await
                    .map_err(|_| invalid("Media file is unavailable"))?;
                file.seek(std::io::SeekFrom::Start(offset))
                    .await
                    .map_err(|_| invalid("Could not seek media file"))?;
                Ok(reqwest::Body::wrap_stream(ReaderStream::new(
                    file.take(length),
                )))
            }
        }
    }
}

#[derive(Clone)]
pub struct Media {
    pub name: String,
    pub mime: String,
    pub source: MediaSource,
    pub public_url: Option<String>,
    pub alt_text: String,
}
#[derive(Clone)]
pub struct Publication {
    pub key: String,
    pub text: String,
    pub title: String,
    pub media: Vec<Media>,
    pub options: Value,
}
#[derive(Clone)]
pub enum Step {
    Checkpoint(Value),
    Wait(Value),
    Published {
        external_id: String,
        external_url: Option<String>,
        receipt: Value,
    },
}
fn received_offset(response: &reqwest::Response) -> Result<u64> {
    match response.headers().get("range") {
        None => Ok(0),
        Some(value) => value
            .to_str()
            .ok()
            .and_then(|v| v.strip_prefix("bytes=0-"))
            .and_then(|v| v.parse::<u64>().ok())
            .and_then(|v| v.checked_add(1))
            .ok_or_else(|| {
                error(
                    FailureKind::Uncertain,
                    "Invalid YouTube upload range receipt",
                )
            }),
    }
}
fn published(id: String, url: Option<String>, receipt: Value) -> Step {
    Step::Published {
        external_id: id,
        external_url: url,
        receipt,
    }
}
fn public_media(m: &Media) -> Result<&str> {
    m.public_url
        .as_deref()
        .ok_or_else(|| invalid("This format requires a temporary public HTTPS media URL"))
}
fn id_path(id: &str) -> String {
    url::form_urlencoded::byte_serialize(id.as_bytes()).collect()
}
fn image(m: &Media) -> bool {
    m.mime.starts_with("image/")
}
fn video(m: &Media) -> bool {
    m.mime.starts_with("video/")
}
fn x_alt_text<'a>(p: &'a Publication, m: &'a Media) -> &'a str {
    if m.alt_text.trim().is_empty() {
        p.options["alt_text"].as_str().unwrap_or("")
    } else {
        &m.alt_text
    }
}
async fn multipart(m: &Media, field_name: &str) -> Result<Form> {
    Ok(Form::new().part(
        field_name.to_owned(),
        Part::stream_with_length(m.source.body(0, m.source.len()).await?, m.source.len())
            .file_name(m.name.clone())
            .mime_str(&m.mime)
            .map_err(|_| invalid("Invalid media content type"))?,
    ))
}
fn post_limit(p: &Publication, max: usize) -> Result<()> {
    if p.text.chars().count() > max {
        return Err(invalid("Text exceeds this provider's supported length"));
    }
    Ok(())
}
mod inspection;
mod reconcile;
mod validation;
pub use inspection::inspect_media;
pub use validation::validate;
mod bluesky;
mod facebook;
mod linkedin;
mod mastodon;
mod meta;
mod pinterest;
mod x;
mod youtube;
impl Adapter {
    /// Every checkpoint must be persisted before the next call.
    pub async fn publish_step(
        &self,
        c: &Channel,
        p: &Publication,
        checkpoint: Option<&Value>,
    ) -> Result<Step> {
        require_permission(
            c,
            if p.media.is_empty() {
                "publish"
            } else {
                "media"
            },
        )?;
        if c.app.provider == "facebook" && p.options["format"] == "reel" {
            require_permission(c, "reel")?;
        }
        validate(&c.app.provider, p)?;
        inspect_media(&c.app.provider, p).await?;
        let empty = json!({});
        let cp = checkpoint.unwrap_or(&empty);
        match c.app.provider.as_str() {
            "x" => self.publish_x(c, p, cp).await,
            "youtube" => self.publish_youtube(c, p, cp).await,
            "facebook" => self.publish_facebook(c, p, cp).await,
            "instagram" | "threads" => self.publish_meta(c, p, cp).await,
            "mastodon" => self.publish_mastodon(c, p, cp).await,
            "bluesky" => self.publish_bluesky(c, p, cp).await,
            "linkedin" => self.publish_linkedin(c, p, cp).await,
            "pinterest" => self.publish_pinterest(c, p, cp).await,
            _ => Err(invalid("Provider has no direct adapter")),
        }
    }
}
