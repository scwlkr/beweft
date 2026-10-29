use anyhow::{Context, Result, bail, ensure};
use reqwest::{Client, Method};
use serde_json::Value;
use std::{io::Read, path::Path, time::Duration};
use tokio_util::io::ReaderStream;
use url::Url;

pub struct Api {
    client: Client,
    origin: Url,
    token: String,
}
// Only local strings are displayed. A remote endpoint can put credentials or
// arbitrary instructions in its error message or even in an unknown error code.
fn known_guidance(status: u16, bytes: &[u8]) -> Option<&'static str> {
    let body: Value = serde_json::from_slice(bytes).ok()?;
    let code = body.get("error")?.get("code")?.as_str()?;
    match (status, code) {
        (400, "post_text_too_long") => {
            Some("Post and channel variant text must each be at most 100,000 bytes.")
        }
        (400, "unsupported_media_type") => Some("Choose JPEG, PNG, GIF, WebP, MP4, or WebM media."),
        (400, "missing_posting_schedule") => {
            Some("Add a posting schedule in the workspace or choose an exact date.")
        }
        (409, "posting_slots_unavailable") => Some(
            "No posting slot is available in the next 60 days. Adjust the posting schedule or choose an exact date.",
        ),
        (409, "post_version_conflict") => Some(
            "The draft changed. Read the current post version before submitting a new request.",
        ),
        (400, "invalid_cursor") => Some(
            "The cursor is invalid or incompatible. Restart the listing with a valid limit, the same filters and authorized brand scope.",
        ),
        (400, "invalid_page_limit") => Some("Use a page limit from 1 to 10."),
        (401, "unauthorized") => {
            Some("Authentication failed. Check the credentials required for this request.")
        }
        (403, "forbidden") => Some(
            "Access is denied. Check token scopes, authorized brands, and whether an owner session is required.",
        ),
        _ => None,
    }
}

fn validate_page_limit(method: &Method, url: &Url) -> Result<()> {
    if method != Method::GET
        || !matches!(
            url.path(),
            "/api/v1/posts" | "/api/v1/media" | "/api/v1/publications"
        )
    {
        return Ok(());
    }
    let mut seen = false;
    for (key, value) in url.query_pairs() {
        if key != "limit" {
            continue;
        }
        ensure!(
            !seen
                && !value.is_empty()
                && value.bytes().all(|byte| byte.is_ascii_digit())
                && value
                    .parse::<u32>()
                    .is_ok_and(|limit| (1..=10).contains(&limit)),
            "Provide limit once as an integer from 1 to 10"
        );
        seen = true;
    }
    Ok(())
}

impl Api {
    pub fn from_env() -> Result<Self> {
        let origin = Url::parse(
            &std::env::var("BEWEFT_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".into()),
        )
        .map_err(|_| anyhow::anyhow!("BEWEFT_URL must be an HTTP(S) origin"))?;
        ensure!(
            matches!(origin.scheme(), "http" | "https")
                && origin.host_str().is_some()
                && origin.username().is_empty()
                && origin.password().is_none()
                && origin.path() == "/"
                && origin.query().is_none()
                && origin.fragment().is_none(),
            "BEWEFT_URL must be an HTTP(S) origin without credentials, path, or query"
        );
        let loopback = matches!(origin.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        ensure!(
            origin.scheme() == "https"
                || loopback
                || std::env::var("BEWEFT_ALLOW_INTERNAL_HTTP").as_deref() == Ok("true"),
            "Use HTTPS for bearer tokens; BEWEFT_ALLOW_INTERNAL_HTTP=true is only for a trusted private container network"
        );
        let token = match (
            std::env::var("BEWEFT_TOKEN").ok(),
            std::env::var("BEWEFT_TOKEN_FILE").ok(),
        ) {
            (Some(_), Some(_)) => bail!("Set only one of BEWEFT_TOKEN or BEWEFT_TOKEN_FILE"),
            (Some(token), None) => token,
            (None, Some(path)) => {
                let mut options = std::fs::OpenOptions::new();
                options.read(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.custom_flags(libc::O_NONBLOCK);
                }
                let file = options.open(path).context("Cannot open token file")?;
                let metadata = file.metadata().context("Cannot read token file metadata")?;
                ensure!(
                    metadata.is_file() && metadata.len() <= 4096,
                    "Token file must be a regular file of at most 4096 bytes"
                );
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    ensure!(
                        metadata.permissions().mode() & 0o077 == 0,
                        "Token file must not be accessible by group or other users (chmod 600)"
                    );
                }
                let mut bytes = Vec::new();
                file.take(4097)
                    .read_to_end(&mut bytes)
                    .context("Cannot read token file")?;
                ensure!(bytes.len() <= 4096, "Token file exceeds 4096 bytes");
                String::from_utf8(bytes)
                    .context("Token file must be UTF-8")?
                    .trim()
                    .into()
            }
            _ => String::new(),
        };
        ensure!(
            token.len() <= 4096 && !token.chars().any(char::is_control),
            "Invalid API token format"
        );
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .build()?;
        Ok(Self {
            client,
            origin,
            token,
        })
    }
    fn endpoint(&self, path: &str) -> Result<Url> {
        // No URL joining: encoded paths, dot segments and network-path references
        // must never redirect an authenticated escape-hatch request off the API.
        ensure!(
            path.starts_with('/')
                && !path.starts_with("//")
                && !path.contains(['\\', '#', '%'])
                && !path.chars().any(char::is_control),
            "Use an API path such as /posts; URLs and encoded paths are forbidden"
        );
        let (route, query) = path.split_once('?').unwrap_or((path, ""));
        ensure!(
            !route.split('/').any(|p| matches!(p, "." | "..")),
            "Dot segments are forbidden"
        );
        let mut url = self.origin.clone();
        url.set_path(&format!("/api/v1{route}"));
        if !query.is_empty() {
            url.set_query(Some(query));
        }
        Ok(url)
    }
    pub async fn request(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value> {
        ensure!(
            !self.token.is_empty(),
            "Set BEWEFT_TOKEN_FILE or BEWEFT_TOKEN to a scoped API token"
        );
        let is_write = method != Method::GET;
        let endpoint = self.endpoint(path)?;
        validate_page_limit(&method, &endpoint)?;
        let mut request = self
            .client
            .request(method, endpoint)
            .bearer_auth(&self.token)
            .header("X-Beweft-Request", "1");
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.map_err(|_| {
            if is_write {
                anyhow::anyhow!("API request failed or timed out; outcome may be unknown. Read authoritative state before retrying a write.")
            } else {
                anyhow::anyhow!("API request failed or timed out")
            }
        })?;
        self.response(response, is_write).await
    }
    async fn response(&self, mut response: reqwest::Response, is_write: bool) -> Result<Value> {
        let status = response.status();
        if status.is_redirection() {
            if is_write {
                bail!(
                    "API returned a redirect; it was not followed. Outcome may be unknown; inspect authoritative state before retrying a write"
                );
            }
            bail!("API returned a redirect; it was not followed");
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| {
            if is_write {
                anyhow::anyhow!("API response was interrupted; outcome may be unknown. Inspect authoritative state before retrying a write")
            } else {
                anyhow::anyhow!("API response was interrupted")
            }
        })? {
            if bytes.len() + chunk.len() > 16 * 1024 * 1024 {
                if is_write {
                    bail!("API response exceeds 16 MiB; outcome may be unknown. Inspect authoritative state before retrying a write");
                }
                bail!("API response exceeds 16 MiB; narrow the query");
            }
            bytes.extend_from_slice(&chunk);
        }
        if !status.is_success() {
            if let Some(guidance) = known_guidance(status.as_u16(), &bytes) {
                // Authentication can fail after an auth mutation has committed
                // (for example, bootstrap before session creation). A 401 is
                // therefore not proof that every write was rejected.
                if is_write && status.as_u16() == 401 {
                    bail!(
                        "API returned HTTP 401: {guidance} Outcome may be unknown. Inspect authoritative state before retrying a write"
                    );
                }
                bail!("API returned HTTP {}: {guidance}", status.as_u16());
            }
            if is_write {
                bail!(
                    "API returned HTTP {}; outcome may be unknown. Inspect authoritative state before retrying a write",
                    status.as_u16()
                );
            }
            bail!("API returned HTTP {}", status.as_u16());
        }
        serde_json::from_slice(&bytes).with_context(|| if is_write {
            "API returned invalid JSON; outcome may be unknown. Inspect authoritative state before retrying a write"
        } else { "API returned invalid JSON" })
    }
    pub async fn upload(&self, brand: &str, path: &Path, file: std::fs::File) -> Result<Value> {
        ensure!(
            !self.token.is_empty(),
            "Set BEWEFT_TOKEN_FILE or BEWEFT_TOKEN"
        );
        let metadata = file.metadata().context("Cannot inspect upload file")?;
        ensure!(metadata.is_file(), "Upload must be a regular file");
        let limits = self.request(Method::GET, "/media/limits", None).await?;
        let max = limits["data"]["max_upload_bytes"]
            .as_u64()
            .context("API omitted upload limit")?;
        ensure!(metadata.len() <= max, "File exceeds server upload limit");
        let filename = path
            .file_name()
            .context("Upload path needs a file name")?
            .to_string_lossy()
            .into_owned();
        let stream = ReaderStream::new(tokio::fs::File::from_std(file));
        let part = reqwest::multipart::Part::stream_with_length(
            reqwest::Body::wrap_stream(stream),
            metadata.len(),
        )
        .file_name(filename);
        let form = reqwest::multipart::Form::new().part("file", part);
        let response = self
            .client
            .post(self.endpoint(&format!("/media?brand_id={brand}"))?)
            // The API permits 15 minutes to receive an upload. Include a
            // bounded margin for connection setup and its response.
            .timeout(Duration::from_secs(16 * 60))
            .bearer_auth(&self.token)
            .header("X-Beweft-Request", "1")
            .multipart(form)
            .send()
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "Upload failed; outcome may be unknown; inspect media before retrying"
                )
            })?;
        self.response(response, true).await
    }
}
