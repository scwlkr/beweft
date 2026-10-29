use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{path::PathBuf, sync::Arc};
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
}
pub struct Config {
    pub public_url: String,
    pub secure_cookie: bool,
    pub setup_secret: String,
    pub media_dir: PathBuf,
    pub fixture_enabled: bool,
    pub max_upload_bytes: u64,
    pub media_quota_bytes: u64,
    pub fixture_delay_ms: u64,
    pub encryption_key: Option<[u8; 32]>,
}
impl AppState {
    pub async fn from_env() -> anyhow::Result<Self> {
        let public_url =
            std::env::var("BEWEFT_PUBLIC_URL").unwrap_or_else(|_| "http://localhost:3000".into());
        let parsed = url::Url::parse(&public_url)?;
        anyhow::ensure!(
            matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some(),
            "BEWEFT_PUBLIC_URL must be an HTTP(S) origin"
        );
        anyhow::ensure!(
            parsed.path() == "/"
                && parsed.query().is_none()
                && parsed.fragment().is_none()
                && parsed.username().is_empty()
                && parsed.password().is_none(),
            "BEWEFT_PUBLIC_URL must be an origin without path or credentials"
        );
        let key = std::env::var("BEWEFT_ENCRYPTION_KEY")
            .ok()
            .map(|s| -> anyhow::Result<[u8; 32]> {
                let bytes = hex::decode(s)?;
                bytes.try_into().map_err(|_| {
                    anyhow::anyhow!(
                        "BEWEFT_ENCRYPTION_KEY must contain 32 bytes encoded as 64 hex characters"
                    )
                })
            })
            .transpose()?;
        let config = Config {
            public_url: parsed.origin().ascii_serialization(),
            secure_cookie: parsed.scheme() == "https",
            setup_secret: std::env::var("BEWEFT_SETUP_SECRET").unwrap_or_default(),
            media_dir: std::env::var("BEWEFT_MEDIA_DIR")
                .unwrap_or_else(|_| ".local/media".into())
                .into(),
            fixture_enabled: std::env::var("BEWEFT_ENABLE_FIXTURES").as_deref() == Ok("true"),
            max_upload_bytes: env_u64("BEWEFT_MAX_UPLOAD_BYTES", 256 * 1024 * 1024)?,
            media_quota_bytes: env_u64("BEWEFT_MEDIA_QUOTA_BYTES", 10 * 1024 * 1024 * 1024)?,
            fixture_delay_ms: env_u64("BEWEFT_FIXTURE_DELAY_MS", 0)?.min(240_000),
            encryption_key: key,
        };
        tokio::fs::create_dir_all(&config.media_dir).await?;
        let db = PgPoolOptions::new()
            .max_connections(10)
            .connect(&std::env::var("DATABASE_URL")?)
            .await?;
        sqlx::migrate!("../../migrations").run(&db).await?;
        Ok(Self {
            db,
            config: Arc::new(config),
        })
    }
}

fn env_u64(key: &str, default: u64) -> anyhow::Result<u64> {
    match std::env::var(key) {
        Ok(value) => {
            let parsed = value.parse::<u64>()?;
            anyhow::ensure!(parsed <= i64::MAX as u64, "{key} is too large");
            Ok(parsed)
        }
        Err(_) => Ok(default),
    }
}
