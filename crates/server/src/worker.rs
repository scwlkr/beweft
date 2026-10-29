use crate::state::AppState;
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;
/// Recover only by quarantine. A lease expiring never authorizes a second external call.
pub async fn recover_stale(state: &AppState) -> anyhow::Result<()> {
    let mut tx = state.db.begin().await?;
    let ids:Vec<Uuid>=sqlx::query_scalar("UPDATE publications SET state='uncertain',last_error='Delivery outcome is unknown. Ensure the original worker has stopped and inspect the provider before retrying.',lease_until=NULL,updated_at=clock_timestamp() WHERE state='sending' AND lease_until<=clock_timestamp() RETURNING id").fetch_all(&mut *tx).await?;
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
        "UPDATE publications SET state='sending',lease_until=clock_timestamp()+interval '5 minutes',updated_at=clock_timestamp() WHERE id=$1",
    )
    .bind(id)
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
    // Direct adapters run only after the durable attempt and lease are committed.
    let (outcome, error, receipt, external_id, external_url) = match provider.as_str() {
        "manual" => (
            "manual_required",
            None,
            json!({"execution":"manual","verification":"awaiting_owner"}),
            None,
            None,
        ),
        "fixture" if state.config.fixture_enabled => (
            "fixture_complete",
            None,
            json!({"execution":"fixture","verification":"fixture_only","external_publication":false}),
            None,
            None,
        ),
        _ if beweft_providers::known(&provider) => {
            match crate::provider_bridge::publish(
                state,
                &json!({"id":id,"attempt_id":attempt,"channel_id":channel}),
            )
            .await
            {
                crate::provider_bridge::BridgeOutcome::Published {
                    external_id,
                    external_url,
                    receipt,
                } => (
                    "published",
                    None,
                    json!({"execution":"direct","verification":"unverified_live","provider_receipt":receipt}),
                    Some(external_id),
                    external_url,
                ),
                crate::provider_bridge::BridgeOutcome::Uncertain { reason } => (
                    "uncertain",
                    Some(reason),
                    json!({"execution":"direct","verification":"outcome_unknown"}),
                    None,
                    None,
                ),
                crate::provider_bridge::BridgeOutcome::Rejected { reason } => (
                    "failed",
                    Some(reason),
                    json!({"execution":"direct","verification":"rejected"}),
                    None,
                    None,
                ),
            }
        }
        _ => (
            "failed",
            Some("No enabled publishing adapter is available".to_owned()),
            json!({"external_request_sent":false}),
            None,
            None,
        ),
    };
    finish_attempt(
        state,
        id,
        attempt,
        outcome,
        error.as_deref(),
        receipt,
        external_id
            .as_deref()
            .map(|id| (id, external_url.as_deref())),
    )
    .await?;
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
    external: Option<(&str, Option<&str>)>,
) -> anyhow::Result<bool> {
    let mut tx = state.db.begin().await?;
    let row = sqlx::query("SELECT state,COALESCE(lease_until>clock_timestamp(),false) AS lease_live FROM publications WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    if row.get::<String, _>("state") != "sending" || !row.get::<bool, _>("lease_live") {
        tx.commit().await?;
        tracing::warn!(publication_id=%id,attempt_id=%attempt,"ignored completion outside its active lease");
        return Ok(false);
    }
    let changed=sqlx::query("UPDATE publication_attempts a SET state=$3,error=$4,receipt=$5,finished_at=clock_timestamp() FROM publications p WHERE a.id=$1 AND a.publication_id=$2 AND a.state='sending' AND p.id=a.publication_id AND p.state='sending' AND p.lease_until>clock_timestamp()").bind(attempt).bind(id).bind(outcome).bind(error).bind(receipt).execute(&mut *tx).await?;
    if changed.rows_affected() != 1 {
        tx.commit().await?;
        tracing::warn!(publication_id=%id,attempt_id=%attempt,"ignored completion from an obsolete attempt");
        return Ok(false);
    }
    let updated = sqlx::query("UPDATE publications SET state=$2,last_error=$3,lease_until=NULL,external_id=CASE WHEN $2='published' THEN $4 ELSE external_id END,external_url=CASE WHEN $2='published' THEN $5 ELSE external_url END,updated_at=clock_timestamp() WHERE id=$1 AND state='sending' AND lease_until>clock_timestamp()").bind(id).bind(outcome).bind(error).bind(external.map(|value| value.0)).bind(external.and_then(|value| value.1)).execute(&mut *tx).await?;
    if updated.rows_affected() != 1 {
        tx.rollback().await?;
        tracing::warn!(publication_id=%id,attempt_id=%attempt,"ignored completion after its lease expired");
        return Ok(false);
    }
    tx.commit().await?;
    Ok(true)
}
pub async fn run(state: AppState, id: String) -> anyhow::Result<()> {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
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
