use super::delivery::save_attempt_checkpoint_tx;
use super::preparation::{checkpoint_on, load_publication_on};
use super::*;

/// A stage holds the claim's publication and channel locks through its network work.
/// Reconnect/recovery cannot replace its grant/attempt between the check and the write.
async fn lock_attempt(
    _state: &AppState,
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: Uuid,
    attempt: Uuid,
    channel_id: Uuid,
) -> Result<(Value, chrono::DateTime<chrono::Utc>, std::time::Duration)> {
    crate::schedules::lock_channel(tx, channel_id).await?;
    let row=sqlx::query("SELECT to_jsonb(p) AS publication,p.state,p.channel_id,p.lease_until FROM publications p WHERE id=$1 FOR UPDATE").bind(id).fetch_one(&mut **tx).await?;
    let active:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM publication_attempts WHERE id=$1 AND publication_id=$2 AND state='sending')").bind(attempt).bind(id).fetch_one(&mut **tx).await?;
    let lease: Option<chrono::DateTime<chrono::Utc>> = row.get("lease_until");
    let lease =
        lease.ok_or_else(|| ApiError::conflict("Publication has no active delivery lease"))?;
    let deadline = (lease - chrono::Duration::seconds(1))
        .min(chrono::Utc::now() + chrono::Duration::seconds(75));
    let remaining = (deadline - chrono::Utc::now())
        .to_std()
        .ok()
        .filter(|d| !d.is_zero());
    if row.get::<String, _>("state") != "sending"
        || row.get::<Uuid, _>("channel_id") != channel_id
        || !active
        || remaining.is_none()
    {
        return Err(ApiError::conflict(
            "Delivery attempt expired or was replaced before its provider request",
        ));
    }
    Ok((row.get("publication"), deadline, remaining.unwrap()))
}

async fn fenced_step(
    state: &AppState,
    id: Uuid,
    attempt: Uuid,
    channel_id: Uuid,
) -> Result<std::result::Result<Step, providers::ProviderError>> {
    // Successful refresh is committed independently before media/checkpoint/probe failures.
    let mut refresh_tx = state.db.begin().await?;
    let (value, deadline, _) =
        lock_attempt(state, &mut refresh_tx, id, attempt, channel_id).await?;
    let refreshed =
        connections::load_channel_tx(state, &mut refresh_tx, channel_id, Some(deadline)).await;
    refresh_tx.commit().await?;
    refreshed?;
    // Capability URLs must already be visible to the provider's separate callback connection.
    let p = {
        let mut db = state.db.acquire().await?;
        load_publication_on(state, &mut db, &value).await?
    };
    let mut tx = state.db.begin().await?;
    let (_, deadline, remaining) = lock_attempt(state, &mut tx, id, attempt, channel_id).await?;
    let work = async {
        let channel = connections::load_stored_channel_tx(state, &mut tx, channel_id).await?;
        let adapter = connections::adapter_for(&channel.app)
            .await?
            .with_deadline(deadline);
        let cp = checkpoint_on(state, &mut tx, id).await?;
        let mut result = adapter.publish_step(&channel, &p, cp.as_ref()).await;
        if let Err(e) = &result {
            if e.kind == FailureKind::Authentication {
                // The channel row has stayed locked since this exact grant was loaded.
                // A concurrent reconnect commits afterward and cannot be invalidated here.
                sqlx::query("UPDATE channels SET connection_status='reconnect_required',connection_error=$2 WHERE id=$1").bind(channel_id).bind(&e.message).execute(&mut *tx).await?;
            }
            if matches!(e.kind, FailureKind::Uncertain | FailureKind::Unavailable)
                && let Some(saved) = cp.as_ref()
                && let Ok(Some(reconciled)) = adapter.reconcile(&channel, &p, saved).await
            {
                result = Ok(reconciled);
            }
        }
        if let Ok(Step::Checkpoint(next) | Step::Wait(next)) = &result {
            save_attempt_checkpoint_tx(&mut tx, state, id, attempt, next).await?;
        }
        Ok::<_, ApiError>(result)
    };
    let result = tokio::time::timeout(remaining, work).await;
    match result {
        Ok(result) => {
            tx.commit().await?;
            result
        }
        Err(_) => Err(ApiError::conflict(
            "Provider stage exceeded its delivery deadline; its outcome is uncertain and must be reconciled",
        )),
    }
}
pub async fn publish(state: &AppState, value: &Value) -> BridgeOutcome {
    let ids = || -> Result<(Uuid, Uuid, Uuid)> {
        Ok((
            crate::error::uuid(value, "id")?,
            crate::error::uuid(value, "attempt_id")?,
            crate::error::uuid(value, "channel_id")?,
        ))
    };
    let (id, attempt, channel) = match ids() {
        Ok(v) => v,
        Err(e) => return BridgeOutcome::Rejected { reason: e.2 },
    };
    let began = std::time::Instant::now();
    for _ in 0..48 {
        if began.elapsed().as_secs() > 180 {
            break;
        }
        match fenced_step(state, id, attempt, channel).await {
            Ok(Ok(Step::Checkpoint(_))) => {}
            Ok(Ok(Step::Wait(_))) => tokio::time::sleep(std::time::Duration::from_secs(3)).await,
            Ok(Ok(Step::Published {
                external_id,
                external_url,
                receipt,
            })) => {
                return BridgeOutcome::Published {
                    external_id,
                    external_url,
                    receipt,
                };
            }
            Ok(Err(e))
                if matches!(
                    e.kind,
                    FailureKind::Uncertain | FailureKind::Unavailable | FailureKind::ExpiredSession
                ) =>
            {
                return BridgeOutcome::Uncertain { reason: e.message };
            }
            Ok(Err(e)) => return BridgeOutcome::Rejected { reason: e.message },
            Err(e) => return BridgeOutcome::Uncertain { reason: e.2 },
        }
    }
    BridgeOutcome::Uncertain {
        reason: "Provider processing needs reconciliation before continuing".into(),
    }
}
