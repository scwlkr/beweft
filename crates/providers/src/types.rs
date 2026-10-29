use crate::*;
#[derive(Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub provider: String,
    pub client_id: String,
    pub client_secret: String,
    /// Only server-admin configuration can supply this (Mastodon instance/PDS).
    #[serde(default)]
    pub instance_url: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_at: Option<i64>,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub scopes_known: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Identity {
    /// Parent account selected during Google Business Profile discovery.
    #[serde(default)]
    pub parent: Option<String>,
    pub id: String,
    pub name: String,
    pub username: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Channel {
    pub identity: Identity,
    pub tokens: Tokens,
    pub app: AppConfig,
}
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    Rejected,
    Authentication,
    RateLimited,
    Uncertain,
    Unavailable,
    ExpiredSession,
}
#[derive(Clone, Debug, Serialize)]
pub struct ProviderError {
    pub kind: FailureKind,
    pub message: String,
    pub status: Option<u16>,
}
impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for ProviderError {}
pub type Result<T> = std::result::Result<T, ProviderError>;
pub fn error(kind: FailureKind, message: &str) -> ProviderError {
    ProviderError {
        kind,
        message: message.into(),
        status: None,
    }
}
pub(crate) fn invalid(message: &str) -> ProviderError {
    error(FailureKind::Rejected, message)
}

impl AppConfig {
    /// Secrets may rotate, but an authorization grant cannot move between clients or origins.
    pub fn same_grant_authority(&self, other: &Self) -> bool {
        self.provider == other.provider
            && self.client_id == other.client_id
            && self
                .instance_url
                .as_deref()
                .map(|s| s.trim_end_matches('/'))
                == other
                    .instance_url
                    .as_deref()
                    .map(|s| s.trim_end_matches('/'))
    }
}
impl Channel {
    /// The selected parent is part of the encrypted credential, while the
    /// destination remains the stable location resource across parent changes.
    pub fn selection_key(&self) -> Result<String> {
        if self.app.provider != "google_business" {
            return Ok(self.identity.id.clone());
        }
        let parent = self
            .identity
            .parent
            .as_deref()
            .ok_or_else(|| invalid("Google account parent is missing"))?;
        google_resource(parent, "accounts")?;
        google_resource(&self.identity.id, "locations")?;
        Ok(format!("{parent}/{}", self.identity.id))
    }
    pub fn destination_key(&self) -> String {
        match &self.app.instance_url {
            Some(origin) => format!("{}|{}", origin.trim_end_matches('/'), self.identity.id),
            None => self.identity.id.clone(),
        }
    }
}

/// Google resource path segments are used only after rejecting separators,
/// escapes and path traversal. Do not accept arbitrary provider-supplied URLs.
pub fn google_resource<'a>(value: &'a str, kind: &str) -> Result<&'a str> {
    let id = value
        .strip_prefix(&format!("{kind}/"))
        .ok_or_else(|| invalid("Invalid Google resource name"))?;
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(invalid("Invalid Google resource name"));
    }
    Ok(id)
}
