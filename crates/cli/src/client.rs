use anyhow::{Context, Result, bail, ensure};
use reqwest::{Client, Method};
use serde_json::Value;
use std::{path::Path, time::Duration};
use url::Url;

pub struct Api {
    client: Client,
    origin: Url,
    token: String,
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
                let metadata =
                    std::fs::metadata(&path).context("Cannot read token file metadata")?;
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
                std::fs::read_to_string(path)
                    .context("Cannot read token file")?
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
        let mut request = self
            .client
            .request(method, self.endpoint(path)?)
            .bearer_auth(&self.token)
            .header("X-Beweft-Request", "1");
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.map_err(|_| anyhow::anyhow!("API request failed or timed out; outcome may be unknown. Read authoritative state before retrying a write."))?;
        self.response(response).await
    }
    async fn response(&self, mut response: reqwest::Response) -> Result<Value> {
        let status = response.status();
        ensure!(
            !status.is_redirection(),
            "API returned a redirect; it was not followed"
        );
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| {
            anyhow::anyhow!(
                "API response was interrupted; inspect authoritative state before retrying a write"
            )
        })? {
            ensure!(
                bytes.len() + chunk.len() <= 16 * 1024 * 1024,
                "API response exceeds 16 MiB; narrow the query"
            );
            bytes.extend_from_slice(&chunk);
        }
        let value: Value = serde_json::from_slice(&bytes).context("API returned invalid JSON")?;
        if !status.is_success() {
            // Deliberately omit server messages: a remote endpoint can echo a secret.
            bail!(
                "API returned HTTP {}; inspect the application for details",
                status.as_u16()
            );
        }
        Ok(value)
    }
    pub async fn upload(&self, brand: &str, path: &Path) -> Result<Value> {
        ensure!(
            !self.token.is_empty(),
            "Set BEWEFT_TOKEN_FILE or BEWEFT_TOKEN"
        );
        let metadata = tokio::fs::metadata(path)
            .await
            .context("Cannot inspect upload file")?;
        ensure!(metadata.is_file(), "Upload must be a regular file");
        let limits = self.request(Method::GET, "/media/limits", None).await?;
        let max = limits["data"]["max_upload_bytes"]
            .as_u64()
            .context("API omitted upload limit")?;
        ensure!(metadata.len() <= max, "File exceeds server upload limit");
        let form = reqwest::multipart::Form::new()
            .file("file", path)
            .await
            .context("Cannot read upload file")?;
        let response = self
            .client
            .post(self.endpoint(&format!("/media?brand_id={brand}"))?)
            .bearer_auth(&self.token)
            .header("X-Beweft-Request", "1")
            .multipart(form)
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("Upload failed; inspect media before retrying"))?;
        self.response(response).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_cannot_escape_origin_or_api() {
        let api = Api {
            client: Client::new(),
            origin: Url::parse("https://example.invalid").unwrap(),
            token: "test".into(),
        };
        for path in [
            "https://evil.invalid",
            "//evil.invalid",
            "/../auth",
            "/%2e%2e/auth",
            "/x\\y",
            "/x#fragment",
            "/\n",
        ] {
            assert!(api.endpoint(path).is_err(), "{path}");
        }
        assert_eq!(
            api.endpoint("/posts?brand_id=abc").unwrap().as_str(),
            "https://example.invalid/api/v1/posts?brand_id=abc"
        );
    }
}
