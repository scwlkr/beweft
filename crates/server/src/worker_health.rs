use anyhow::{Result, bail, ensure};
use sqlx::{Connection, PgConnection, postgres::PgConnectOptions};
use std::{env::VarError, time::Duration};
use url::Url;

/// The ID is used as a database key and in diagnostics; never echo bad input.
pub fn configured_id() -> Result<Option<String>> {
    let id = match std::env::var("BEWEFT_WORKER_ID") {
        Ok(id) => id,
        Err(VarError::NotPresent) => return Ok(None),
        Err(VarError::NotUnicode(_)) => bail!("BEWEFT_WORKER_ID must be ASCII"),
    };
    ensure!(
        !id.is_empty()
            && id.len() <= 128
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')),
        "BEWEFT_WORKER_ID must contain 1 to 128 ASCII letters, digits, '.', '_' or '-'"
    );
    Ok(Some(id))
}

/// Remove libpq-style ambient configuration from this healthcheck process.
///
/// # Safety
/// Call only at process startup before creating any runtime or other threads.
pub unsafe fn isolate_postgres_environment() {
    let keys: Vec<_> = std::env::vars_os()
        .filter_map(|(key, _)| {
            key.to_string_lossy()
                .to_ascii_uppercase()
                .starts_with("PG")
                .then_some(key)
        })
        .collect();
    // SAFETY: the caller guarantees this is the only running thread. This
    // changes only the short-lived healthcheck process, not its parent.
    unsafe {
        for key in keys {
            std::env::remove_var(key);
        }
        // SQLx 0.8.6 uses these defaults before parsing a URL. Supplying them
        // avoids OS username/socket discovery and makes password Some("") so
        // its synchronous apply_pgpass never opens PGPASSFILE or ~/.pgpass.
        std::env::set_var("PGHOST", "127.0.0.1");
        std::env::set_var("PGUSER", "postgres");
        std::env::set_var("PGPASSWORD", "");
    }
}

/// Parse only explicitly addressed connections. SQLx's URL parser must be
/// called after isolate_postgres_environment, because it normally reads pgpass.
pub fn connection_options(database_url: &str) -> Result<PgConnectOptions> {
    ensure!(database_url.len() <= 16 * 1024, "DATABASE_URL is too long");
    let url = Url::parse(database_url)
        .map_err(|_| anyhow::anyhow!("DATABASE_URL must be a PostgreSQL URL"))?;
    ensure!(
        matches!(url.scheme(), "postgres" | "postgresql"),
        "DATABASE_URL must be a PostgreSQL URL"
    );
    let mut host = url.host_str().is_some_and(|value| !value.is_empty());
    let mut user = !url.username().is_empty();
    let mut database = !url.path().trim_start_matches('/').is_empty();
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "host" | "hostaddr" => host = !value.is_empty(),
            "user" => user = !value.is_empty(),
            "dbname" => database = !value.is_empty(),
            _ => {}
        }
    }
    ensure!(
        host && user && database,
        "DATABASE_URL must explicitly provide a host or socket, user and database"
    );
    database_url
        .parse::<PgConnectOptions>()
        .map_err(|_| anyhow::anyhow!("DATABASE_URL has invalid PostgreSQL options"))
}

/// Bound the entire probe, including synchronous parsing and TLS processing.
pub fn probe(database_url: String, id: String) -> Result<()> {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("beweft-healthcheck".into())
        .spawn(move || {
            let result = (|| {
                let options = connection_options(&database_url)?;
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?;
                let result = runtime.block_on(check(&options, &id));
                // DNS and explicit certificate files may use blocking threads.
                runtime.shutdown_timeout(Duration::from_millis(100));
                result
            })();
            let _ = sender.send(result);
        })
        .map_err(|_| anyhow::anyhow!("Worker healthcheck could not start"))?;
    match receiver.recv_timeout(Duration::from_secs(4)) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => bail!("Worker healthcheck timed out"),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            bail!("Worker healthcheck failed")
        }
    }
}

/// This read-only path deliberately avoids AppState, migrations and scheduler startup.
pub async fn check(options: &PgConnectOptions, id: &str) -> Result<()> {
    let healthy = tokio::time::timeout(Duration::from_secs(3), async {
        let mut db = PgConnection::connect_with(options).await?;
        sqlx::query_scalar::<_, bool>(
            "SELECT COALESCE(pg_catalog.current_schema() = 'public', false) AND EXISTS(SELECT 1 FROM public.worker_heartbeats WHERE id=$1 AND last_seen > pg_catalog.now() - interval '30 seconds')",
        )
        .bind(id)
        .fetch_one(&mut db)
        .await
    })
    .await
    .map_err(|_| anyhow::anyhow!("Worker healthcheck timed out"))?
    .map_err(|_| anyhow::anyhow!("Worker healthcheck could not read the database"))?;
    ensure!(healthy, "Worker heartbeat is missing or stale");
    Ok(())
}
