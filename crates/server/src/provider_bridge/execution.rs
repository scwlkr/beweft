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
) -> Result<(Value, providers::Deadline)> {
    crate::schedules::lock_channel(tx, channel_id).await?;
    let row =
        sqlx::query("SELECT to_jsonb(p) AS publication FROM publications p WHERE id=$1 FOR UPDATE")
            .bind(id)
            .fetch_one(&mut **tx)
            .await?;
    // Take both locks before reading the wall clock: time spent waiting on a
    // lock must never be included in the remaining delivery budget.
    let locked: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM publication_attempts WHERE id=$1 AND publication_id=$2 FOR UPDATE",
    )
    .bind(attempt)
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?;
    let started = providers::SuspendInstant::now();
    let remaining: Option<f64> = if locked.is_some() {
        sqlx::query_scalar("SELECT EXTRACT(EPOCH FROM (p.lease_until-clock_timestamp()))::double precision FROM publications p JOIN publication_attempts a ON a.publication_id=p.id WHERE p.id=$1 AND p.channel_id=$2 AND p.state='sending' AND a.id=$3 AND a.state='sending' AND p.lease_until>clock_timestamp()+interval '1 second'")
            .bind(id).bind(channel_id).bind(attempt).fetch_optional(&mut **tx).await?
    } else {
        None
    };
    let deadline = remaining
        .filter(|seconds| seconds.is_finite() && *seconds > 1.0)
        .and_then(|seconds| {
            started?.deadline(std::time::Duration::from_secs_f64(
                (seconds - 1.0).min(75.0),
            ))
        })
        .filter(|deadline| deadline.remaining().is_some())
        .ok_or_else(|| {
            ApiError::conflict(
                "Delivery attempt expired or was replaced before its provider request",
            )
        })?;
    Ok((row.get("publication"), deadline))
}

async fn admit_stage(
    state: &AppState,
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: Uuid,
    attempt: Uuid,
    channel_id: Uuid,
) -> Result<(Value, providers::Deadline)> {
    let (_, deadline) = lock_attempt(state, tx, id, attempt, channel_id).await?;
    let remaining = deadline.remaining().ok_or_else(|| {
        ApiError::conflict("Delivery lease expired while waiting for the channel")
    })?;
    // Bound the row wait on PostgreSQL itself. Dropping a timed-out client
    // future alone can leave the server query blocked and the pool occupied.
    sqlx::query("SELECT set_config('lock_timeout',$1,true)")
        .bind(format!("{}ms", remaining.as_millis().max(1)))
        .execute(&mut **tx)
        .await?;
    let channel = sqlx::query("SELECT c.paused,b.archived_at IS NOT NULL AS archived FROM channels c JOIN brands b ON b.id=c.brand_id WHERE c.id=$1 FOR UPDATE OF c")
        .bind(channel_id)
        .fetch_one(&mut **tx)
        .await?;
    if channel.get::<bool, _>("paused") || channel.get::<bool, _>("archived") {
        return Err(ApiError::conflict_code(
            "delivery_paused",
            "The channel or brand is paused; no new provider request was started",
        ));
    }
    sqlx::query("SELECT set_config('lock_timeout','0',true)")
        .execute(&mut **tx)
        .await?;
    // The channel row may have been held by a sync. Recheck database time
    // after acquiring it, immediately before any refresh or delivery setup.
    lock_attempt(state, tx, id, attempt, channel_id).await
}

async fn fenced_step(
    state: &AppState,
    id: Uuid,
    attempt: Uuid,
    channel_id: Uuid,
    overall: Option<providers::Deadline>,
) -> Result<std::result::Result<Step, providers::ProviderError>> {
    // Successful refresh is committed independently before media/checkpoint/probe failures.
    let mut refresh_tx = state.db.begin().await?;
    let (value, deadline) = admit_stage(state, &mut refresh_tx, id, attempt, channel_id).await?;
    let refreshed =
        connections::load_channel_tx(state, &mut refresh_tx, channel_id, Some(deadline)).await;
    // The channel remains locked until this commit. A refresh accepted before
    // expiry may rotate its grant, so persist that rotation even if storage is
    // slow; the next stage rechecks the lease before any publication request.
    refresh_tx.commit().await?;
    refreshed?;
    // Capability URLs must already be visible to the provider's separate callback connection.
    let p = {
        let mut db = state.db.acquire().await?;
        load_publication_on(state, &mut db, &value).await?
    };
    let mut tx = state.db.begin().await?;
    let (_, deadline) = admit_stage(state, &mut tx, id, attempt, channel_id).await?;
    let work = async {
        let channel = connections::load_stored_channel_tx(state, &mut tx, channel_id).await?;
        let adapter = connections::adapter_for(&channel.app)
            .await?
            .with_deadline(deadline);
        let mut cp = checkpoint_on(state, &mut tx, id).await?;
        let stage_started: chrono::DateTime<chrono::Utc> =
            sqlx::query_scalar("SELECT clock_timestamp()")
                .fetch_one(&mut *tx)
                .await?;
        let timing = sqlx::query("SELECT COALESCE(not_before>clock_timestamp(),false) AS waiting,COALESCE(expires_at<=clock_timestamp(),false) AS expired,expires_at IS NOT NULL AS has_expiry FROM provider_delivery_checkpoints WHERE publication_id=$1 FOR UPDATE")
            .bind(id).fetch_optional(&mut *tx).await?;
        if let Some(saved) = cp.as_mut() {
            let stage = saved["stage"].as_str().unwrap_or("");
            if channel.app.provider == "x" && stage == "media" {
                // Old image checkpoints cannot prove whether a post was sent.
                saved["stage"] = json!("x_legacy_ambiguous");
            }
            let stage = saved["stage"].as_str().unwrap_or("");
            if channel.app.provider == "x" && stage.starts_with("x_") {
                let needs_expiry = matches!(
                    stage,
                    "x_video_append" | "x_video_finalize" | "x_video_processing" | "x_video_ready"
                ) || (matches!(stage, "x_image_media" | "x_post_started")
                    && saved["media_ids"]
                        .as_array()
                        .is_some_and(|ids| !ids.is_empty()));
                if needs_expiry
                    && timing
                        .as_ref()
                        .is_none_or(|row| !row.get::<bool, _>("has_expiry"))
                {
                    return Ok(Err(providers::error(
                        FailureKind::Uncertain,
                        "X media expiry is missing. Inspect the delivery and reset before retrying.",
                    )));
                }
                if timing
                    .as_ref()
                    .is_some_and(|row| row.get::<bool, _>("expired"))
                {
                    saved["expired"] = json!(true);
                    let _ = save_attempt_checkpoint_tx(
                        &mut tx,
                        state,
                        id,
                        attempt,
                        saved,
                        stage_started,
                        None,
                    )
                    .await?;
                    return Ok(Err(providers::error(
                        FailureKind::ExpiredSession,
                        "X media expired. Inspect the delivery and use the explicit reset before retrying.",
                    )));
                }
                if timing
                    .as_ref()
                    .is_some_and(|row| row.get::<bool, _>("waiting"))
                {
                    return Ok::<_, ApiError>(Ok(Step::Wait(saved.clone())));
                }
                let previous = saved["attempt_id"]
                    .as_str()
                    .and_then(|value| value.parse::<Uuid>().ok());
                if previous != Some(attempt) {
                    let is_public = matches!(stage, "x_post_started" | "x_legacy_ambiguous");
                    let rebound = save_attempt_checkpoint_tx(
                        &mut tx,
                        state,
                        id,
                        attempt,
                        saved,
                        stage_started,
                        None,
                    )
                    .await?;
                    if is_public {
                        return Ok(Err(providers::error(
                            FailureKind::Uncertain,
                            "An earlier X post may have started. Inspect the provider and record its outcome before retrying.",
                        )));
                    }
                    return Ok(Ok(Step::Checkpoint(rebound)));
                }
                // Refresh and lock waits consume real elapsed time too. Do
                // not begin the public call unless its bounded stage still
                // fits inside the original job's remaining time.
                if stage == "x_post_started"
                    && overall
                        .and_then(|deadline| deadline.remaining())
                        .is_none_or(|remaining| remaining < std::time::Duration::from_secs(80))
                {
                    return Ok(Err(providers::error(
                        FailureKind::Uncertain,
                        "The X post boundary needs owner inspection before recovery.",
                    )));
                }
            }
        }
        let mut result = adapter.publish_step(&channel, &p, cp.as_ref()).await;
        // Recheck under the same locks after network I/O, before mutating any
        // health/checkpoint state or beginning a second provider request.
        let (_, next_deadline) = lock_attempt(state, &mut tx, id, attempt, channel_id).await?;
        if let Err(e) = &result {
            if e.kind == FailureKind::Authentication {
                // The channel row has stayed locked since this exact grant was loaded.
                // A concurrent reconnect commits afterward and cannot be invalidated here.
                sqlx::query("UPDATE channels SET connection_status='reconnect_required',connection_error=$2 WHERE id=$1").bind(channel_id).bind(&e.message).execute(&mut *tx).await?;
            }
            if matches!(e.kind, FailureKind::Uncertain | FailureKind::Unavailable)
                && let Some(saved) = cp.as_ref()
                && let Ok(Some(reconciled)) = adapter
                    .clone()
                    .with_deadline(next_deadline)
                    .reconcile(&channel, &p, saved)
                    .await
            {
                result = Ok(reconciled);
                lock_attempt(state, &mut tx, id, attempt, channel_id).await?;
            }
        }
        if let Ok(Step::Checkpoint(next) | Step::Wait(next)) = &mut result {
            let wait = if next["stage"] == "x_video_processing" {
                next["check_after_secs"].as_u64()
            } else {
                None
            };
            *next =
                save_attempt_checkpoint_tx(&mut tx, state, id, attempt, next, stage_started, wait)
                    .await?;
        }
        Ok::<_, ApiError>(result)
    };
    let result = deadline.run(work).await;
    match result {
        Ok(Ok(result)) => {
            tx.commit().await?;
            Ok(result)
        }
        Ok(Err(error)) => {
            tx.rollback().await?;
            Err(error)
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
    let overall = providers::SuspendInstant::now()
        .and_then(|start| start.deadline(std::time::Duration::from_secs(180)));
    let mut completed_x_checkpoint = false;
    let mut post_started = false;
    for step in 0..48 {
        if overall.and_then(|deadline| deadline.remaining()).is_none() {
            if completed_x_checkpoint && !post_started {
                return BridgeOutcome::Continue;
            }
            break;
        }
        match fenced_step(state, id, attempt, channel, overall).await {
            Ok(Ok(Step::Checkpoint(next))) => {
                let stage = next["stage"].as_str().unwrap_or("");
                if stage.starts_with("x_") {
                    completed_x_checkpoint = true;
                    post_started = stage == "x_post_started" || stage == "x_legacy_ambiguous";
                    let remaining = overall
                        .and_then(|deadline| deadline.remaining())
                        .unwrap_or_default();
                    if post_started {
                        if remaining < std::time::Duration::from_secs(80) {
                            return BridgeOutcome::Uncertain { reason: "The X post may have started. Inspect its outcome before retrying.".into() };
                        }
                    } else if step >= 11 || remaining < std::time::Duration::from_secs(80) {
                        return BridgeOutcome::Continue;
                    }
                }
            }
            Ok(Ok(Step::Wait(next))) if next["stage"] == "x_video_processing" => {
                return BridgeOutcome::Continue;
            }
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
            Err(e)
                if e.1 == "delivery_paused"
                    && (step == 0 || (completed_x_checkpoint && !post_started)) =>
            {
                return BridgeOutcome::Continue;
            }
            Err(e) => return BridgeOutcome::Uncertain { reason: e.2 },
        }
    }
    BridgeOutcome::Uncertain {
        reason: "Provider processing needs reconciliation before continuing".into(),
    }
}
