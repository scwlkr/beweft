use anyhow::{Result, bail, ensure};
use sqlx::{Connection, PgConnection};
use std::{env::VarError, time::Duration};

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

/// This read-only path deliberately avoids AppState, migrations and scheduler startup.
pub async fn check(database_url: &str, id: &str) -> Result<()> {
    let healthy = tokio::time::timeout(Duration::from_secs(3), async {
        let mut db = PgConnection::connect(database_url).await?;
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM worker_heartbeats WHERE id=$1 AND last_seen > now() - interval '30 seconds')",
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
