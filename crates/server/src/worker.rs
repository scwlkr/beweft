use crate::state::AppState;
use chrono::{Duration, Utc};
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;
/// Recover only by quarantine. A lease expiring never authorizes a second external call.
pub async fn recover_stale(state: &AppState) -> anyhow::Result<()> {
    let mut tx = state.db.begin().await?;
    let ids:Vec<Uuid>=sqlx::query_scalar("UPDATE publications SET state='uncertain',last_error='Worker stopped while an attempt was in flight. Reconcile before retrying.',lease_until=NULL,updated_at=now() WHERE state='sending' AND lease_until<now() RETURNING id").fetch_all(&mut *tx).await?;
    if !ids.is_empty() {
        sqlx::query("UPDATE publication_attempts SET state='uncertain',finished_at=now(),error='Worker lease expired; external outcome is unknown' WHERE publication_id=ANY($1) AND state='sending'").bind(ids).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}
pub async fn tick(state: &AppState, worker_id: &str) -> anyhow::Result<bool> {
    sqlx::query("INSERT INTO worker_heartbeats(id,last_seen) VALUES($1,now()) ON CONFLICT(id) DO UPDATE SET last_seen=now()").bind(worker_id).execute(&state.db).await?;
    recover_stale(state).await?;
    let mut tx = state.db.begin().await?;
    let candidate: Option<Uuid> = sqlx::query_scalar("SELECT p.channel_id FROM publications p JOIN channels c ON c.id=p.channel_id JOIN brands b ON b.id=c.brand_id WHERE b.archived_at IS NULL AND p.state='queued' AND p.scheduled_at<=now() AND NOT c.paused AND (c.provider IN ('manual','fixture') OR c.connection_status='connected') ORDER BY p.scheduled_at LIMIT 1").fetch_optional(&mut *tx).await?;
    let Some(channel) = candidate else {
        tx.commit().await?;
        return Ok(false);
    };
    let brand: Uuid = sqlx::query_scalar("SELECT brand_id FROM channels WHERE id=$1")
        .bind(channel)
        .fetch_one(&mut *tx)
        .await?;
    crate::brand_admin::lock(&mut tx, brand)
        .await
        .map_err(|e| anyhow::anyhow!("{}", e.2))?;
    crate::schedules::lock_channel(&mut tx, channel)
        .await
        .map_err(|e| anyhow::anyhow!("{}", e.2))?;
    // Re-read after the shared channel lock. A concurrent disconnect/pause may have changed eligibility.
    let Some(row)=sqlx::query("SELECT p.id,p.channel_id,c.provider FROM publications p JOIN channels c ON c.id=p.channel_id JOIN brands b ON b.id=c.brand_id WHERE b.archived_at IS NULL AND p.channel_id=$1 AND p.state='queued' AND p.scheduled_at<=now() AND NOT c.paused AND (c.provider IN ('manual','fixture') OR c.connection_status='connected') ORDER BY p.scheduled_at FOR UPDATE OF p SKIP LOCKED LIMIT 1").bind(channel).fetch_optional(&mut *tx).await? else {tx.commit().await?;return Ok(false);};
    let id: Uuid = row.get("id");
    let provider: String = row.get("provider");
    let attempt = Uuid::new_v4();
    sqlx::query(
        "UPDATE publications SET state='sending',lease_until=$2,updated_at=now() WHERE id=$1",
    )
    .bind(id)
    .bind(Utc::now() + Duration::minutes(5))
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO publication_attempts(id,publication_id,state,provider) VALUES($1,$2,'sending',$3)").bind(attempt).bind(id).bind(&provider).execute(&mut *tx).await?;
    tx.commit().await?;
    if provider == "fixture" && state.config.fixture_enabled && state.config.fixture_delay_ms > 0 {
        tokio::time::sleep(std::time::Duration::from_millis(
            state.config.fixture_delay_ms,
        ))
        .await;
    }
    // No network activity exists in the foundation adapter. Providers integrate here after a durable attempt exists.
    let (outcome, error, receipt) = match provider.as_str() {
        "manual" => (
            "manual_required",
            None,
            json!({"execution":"manual","verification":"awaiting_owner"}),
        ),
        "fixture" if state.config.fixture_enabled => (
            "fixture_complete",
            None,
            json!({"execution":"fixture","verification":"fixture_only","external_publication":false}),
        ),
        _ => (
            "failed",
            Some("No enabled publishing adapter is available"),
            json!({"external_request_sent":false}),
        ),
    };
    finish_attempt(state, id, attempt, outcome, error, receipt).await?;
    Ok(true)
}
/// Lock ordering is publication, then its durable attempt. The attempt identity fences late workers
/// even when an owner has reconciled and retried the same publication into a new sending state.
pub async fn finish_attempt(
    state: &AppState,
    id: Uuid,
    attempt: Uuid,
    outcome: &str,
    error: Option<&str>,
    receipt: serde_json::Value,
) -> anyhow::Result<bool> {
    let mut tx = state.db.begin().await?;
    let row = sqlx::query("SELECT state,lease_until FROM publications WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    let lease = row.get::<Option<chrono::DateTime<Utc>>, _>("lease_until");
    if row.get::<String, _>("state") != "sending" || lease.is_none_or(|until| until <= Utc::now()) {
        tx.commit().await?;
        tracing::warn!(publication_id=%id,attempt_id=%attempt,"ignored completion outside its active lease");
        return Ok(false);
    }
    let changed=sqlx::query("UPDATE publication_attempts SET state=$3,error=$4,receipt=$5,finished_at=now() WHERE id=$1 AND publication_id=$2 AND state='sending'").bind(attempt).bind(id).bind(outcome).bind(error).bind(receipt).execute(&mut *tx).await?;
    if changed.rows_affected() != 1 {
        tx.commit().await?;
        tracing::warn!(publication_id=%id,attempt_id=%attempt,"ignored completion from an obsolete attempt");
        return Ok(false);
    }
    sqlx::query("UPDATE publications SET state=$2,last_error=$3,lease_until=NULL,updated_at=now() WHERE id=$1").bind(id).bind(outcome).bind(error).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(true)
}
pub async fn run(state: AppState) -> anyhow::Result<()> {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let id = format!("worker-{}", Uuid::new_v4());
    let stopped = Arc::new(AtomicBool::new(false));
    let signal_stop = stopped.clone();
    let signal_task = tokio::spawn(async move {
        crate::shutdown().await;
        signal_stop.store(true, Ordering::Relaxed);
    });
    let heartbeat_state = state.clone();
    let heartbeat_id = id.clone();
    let heartbeat = tokio::spawn(async move {
        loop {
            if let Err(error) = sqlx::query("INSERT INTO worker_heartbeats(id,last_seen) VALUES($1,now()) ON CONFLICT(id) DO UPDATE SET last_seen=now()")
                .bind(&heartbeat_id).execute(&heartbeat_state.db).await { tracing::error!(%error, "worker heartbeat failed"); }
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        }
    });
    tracing::info!(worker=%id, "scheduler started");
    while !stopped.load(Ordering::Relaxed) {
        match tick(&state, &id).await {
            Ok(true) => continue,
            Ok(false) => {}
            Err(error) => tracing::error!(%error, "scheduler tick failed"),
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    heartbeat.abort();
    signal_task.abort();
    let _ = heartbeat.await;
    let _ = signal_task.await;
    sqlx::query("DELETE FROM worker_heartbeats WHERE id=$1")
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(())
}
