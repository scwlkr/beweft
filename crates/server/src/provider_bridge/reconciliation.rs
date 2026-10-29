use super::delivery::save_checkpoint_tx;
use super::preparation::{checkpoint, checkpoint_on, load_publication_on};
use super::*;

/// A definite private-upload rejection can be restarted only by the explicit
/// retry transaction after that exact sender has durably finished as rejected.
pub(crate) async fn prepare_bluesky_video_retry_tx(
    state: &AppState,
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: Uuid,
) -> Result<()> {
    let Some(mut cp) = checkpoint_on(state, tx, id).await? else {
        return Ok(());
    };
    if cp["stage"] != "bsky_video_upload_rejected" {
        return Ok(());
    }
    let attempt = cp["attempt_id"]
        .as_str()
        .and_then(|value| value.parse::<Uuid>().ok())
        .ok_or_else(|| {
            ApiError::conflict("The Bluesky rejection has no valid originating attempt")
        })?;
    let confirmed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM provider_delivery_checkpoints cp JOIN publication_attempts a ON a.id=cp.attempt_id AND a.publication_id=cp.publication_id WHERE cp.publication_id=$1 AND cp.attempt_id=$2 AND a.state='failed' AND a.error=$3 AND a.receipt->>'verification'='rejected')")
        .bind(id).bind(attempt).bind(providers::BLUESKY_VIDEO_RATE_LIMIT)
        .fetch_one(&mut **tx).await?;
    if !confirmed {
        return Err(ApiError::conflict(
            "The original Bluesky upload attempt has no completed rejection. Inspect its outcome before recovery.",
        ));
    }
    cp["stage"] = json!("bsky_video_preflight");
    save_checkpoint_tx(tx, state, id, &cp).await?;
    Ok(())
}

/// Legacy X image checkpoints predate the public-write boundary. Their
/// database identities make them observable without a new provider request.
pub(crate) async fn x_checkpoint_on(
    state: &AppState,
    db: &mut sqlx::PgConnection,
    id: Uuid,
) -> Result<Option<Value>> {
    let Some(mut cp) = checkpoint_on(state, db, id).await? else {
        return Ok(None);
    };
    if cp["stage"] == "media" {
        let row = sqlx::query("SELECT checkpoint_id,attempt_id FROM provider_delivery_checkpoints WHERE publication_id=$1")
            .bind(id).fetch_one(&mut *db).await?;
        let attempt: Option<Uuid> = row.get("attempt_id");
        cp["stage"] = json!("x_legacy_ambiguous");
        cp["checkpoint_id"] = json!(row.get::<Uuid, _>("checkpoint_id"));
        cp["attempt_id"] =
            json!(attempt.ok_or_else(|| ApiError::conflict(
                "The legacy X attempt has no recovery identity"
            ))?);
    }
    Ok(Some(cp))
}

pub(crate) fn x_identity(cp: &Value) -> Result<(Uuid, Uuid)> {
    let parse = |name: &str| cp[name].as_str().and_then(|value| value.parse().ok());
    match (parse("attempt_id"), parse("checkpoint_id")) {
        (Some(attempt), Some(checkpoint)) => Ok((attempt, checkpoint)),
        _ => Err(ApiError::conflict(
            "The X checkpoint has no valid recovery identity",
        )),
    }
}
pub(crate) fn x_public_boundary(cp: &Value) -> bool {
    // Unknown/tampered stages cannot prove that the public write did not run.
    !matches!(
        cp["stage"].as_str(),
        Some(
            "x_image_media"
                | "x_video_init"
                | "x_video_append"
                | "x_video_finalize"
                | "x_video_processing"
                | "x_video_ready"
        )
    )
}
fn x_attested(cp: &Value, attempt: Uuid, checkpoint: Uuid) -> bool {
    cp["not_published_attestation"]["attempt_id"] == json!(attempt)
        && cp["not_published_attestation"]["checkpoint_id"] == json!(checkpoint)
}

pub async fn reset_x_upload(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(input): Json<crate::contracts::ResetXUploadRequest>,
) -> Result<Json<Value>> {
    auth.owner_session()?;
    if !input.sender_stopped || !input.confirm_restart {
        return Err(ApiError::bad(
            "Confirm the original sender has stopped and explicitly confirm restart",
        ));
    }
    let channel: Uuid = sqlx::query_scalar("SELECT channel_id FROM publications WHERE id=$1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    let mut tx = state.db.begin().await?;
    crate::schedules::lock_channel(&mut tx, channel).await?;
    let row = sqlx::query("SELECT p.brand_id,p.state,c.provider FROM publications p JOIN channels c ON c.id=p.channel_id WHERE p.id=$1 FOR UPDATE OF p")
        .bind(id).fetch_one(&mut *tx).await?;
    auth.brand(row.get("brand_id"))?;
    let current: String = row.get("state");
    if row.get::<String, _>("provider") != "x"
        || !["uncertain", "failed"].contains(&current.as_str())
    {
        return Err(ApiError::conflict(
            "Only an uncertain or failed X delivery can be reset",
        ));
    }
    let cp = x_checkpoint_on(&state, &mut tx, id)
        .await?
        .ok_or_else(|| ApiError::conflict("There is no X checkpoint to reset"))?;
    let (attempt, checkpoint) = x_identity(&cp)?;
    if (attempt, checkpoint) != (input.observed_attempt_id, input.observed_checkpoint_id) {
        return Err(ApiError::conflict(
            "The X attempt or checkpoint changed. Reload provider status before recovery",
        ));
    }
    if x_public_boundary(&cp)
        && (current != "failed"
            || input.provider_inspected != Some(true)
            || !x_attested(&cp, attempt, checkpoint))
    {
        return Err(ApiError::conflict(
            "Inspect X and record a not-published resolution for this exact attempt and checkpoint before resetting",
        ));
    }
    if !cp["stage"]
        .as_str()
        .is_some_and(|stage| stage.starts_with("x_"))
    {
        return Err(ApiError::conflict("This is not an X delivery checkpoint"));
    }
    sqlx::query("DELETE FROM provider_delivery_checkpoints WHERE publication_id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE publications SET state='failed',lease_until=NULL,updated_at=clock_timestamp() WHERE id=$1").bind(id).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO audit_events(id,owner_id,action,subject_id,detail) VALUES($1,$2,'reset_x_upload',$3,$4)")
        .bind(Uuid::new_v4()).bind(auth.owner_id).bind(id)
        .bind(json!({"attempt_id":attempt,"checkpoint_id":checkpoint,"sender_stopped":true,"confirm_restart":true,"provider_inspected":input.provider_inspected}))
        .execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(data(
        json!({"state":"failed","session_reset":true,"next_action":"Review the publication, then explicitly retry to start again"}),
    ))
}

pub async fn reconcile(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    auth.scope("publishing:write")?;
    let prepared: Value = sqlx::query_scalar(
        "SELECT to_jsonb(p) FROM publications p WHERE id=$1 AND state='uncertain'",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| ApiError::conflict("Only uncertain publications can be reconciled"))?;
    let channel_id = crate::error::uuid(&prepared, "channel_id")?;
    auth.brand(crate::error::uuid(&prepared, "brand_id")?)?;
    // Refresh commits before any checkpoint lookup, status probe or inconclusive return.
    connections::load_channel(&state, channel_id).await?;
    let p = {
        let mut db = state.db.acquire().await?;
        load_publication_on(&state, &mut db, &prepared).await?
    };
    let mut tx = state.db.begin().await?;
    crate::schedules::lock_channel(&mut tx, channel_id).await?;
    let value: Value = sqlx::query_scalar(
        "SELECT to_jsonb(p) FROM publications p WHERE id=$1 AND state='uncertain' FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| ApiError::conflict("Only uncertain publications can be reconciled"))?;
    auth.brand(
        value["brand_id"]
            .as_str()
            .unwrap()
            .parse()
            .map_err(ApiError::internal)?,
    )?;
    // Lock spans the probe and checkpoint transition: a stale result can never replace newer progress.
    let channel = connections::load_stored_channel_tx(&state, &mut tx, channel_id).await?;
    let mut cp = checkpoint_on(&state, &mut tx,id).await?.ok_or_else(||ApiError::bad("No queryable provider receipt exists. Inspect the provider and use owner reconciliation."))?;
    let outcome = connections::adapter_for(&channel.app)
        .await?
        .reconcile(&channel, &p, &cp)
        .await;
    let (target, receipt) = match outcome {
        Ok(Some(Step::Published {
            external_id,
            external_url,
            receipt,
        })) => {
            sqlx::query("UPDATE publications SET external_id=$2,external_url=$3 WHERE id=$1")
                .bind(id)
                .bind(external_id)
                .bind(external_url)
                .execute(&mut *tx)
                .await?;
            ("published", receipt)
        }
        Ok(Some(Step::Checkpoint(next))) => {
            save_checkpoint_tx(&mut tx, &state, id, &next).await?;
            if channel.app.provider == "bluesky" {
                let processing = next["stage"] == "bsky_video_processing";
                sqlx::query("UPDATE provider_delivery_checkpoints SET not_before=CASE WHEN $2 THEN clock_timestamp()+interval '2 seconds' ELSE NULL END,expires_at=CASE WHEN $2 THEN clock_timestamp()+interval '30 minutes' ELSE NULL END WHERE publication_id=$1")
                    .bind(id)
                    .bind(processing)
                    .execute(&mut *tx)
                    .await?;
            }
            (
                "queued",
                json!({"offset":next["offset"],"resuming_existing_session":true}),
            )
        }
        Err(e) if e.kind == FailureKind::ExpiredSession => {
            cp["expired"] = json!(true);
            save_checkpoint_tx(&mut tx, &state, id, &cp).await?;
            sqlx::query("UPDATE publications SET last_error=$2,updated_at=now() WHERE id=$1")
                .bind(id)
                .bind(&e.message)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            return Ok(data(
                json!({"state":"uncertain","session_expired":true,"reason":e.message}),
            ));
        }
        Err(e) => return Err(ApiError::bad(e.message)),
        _ => {
            return Ok(data(
                json!({"state":"uncertain","reason":"No conclusive provider receipt was found. Inspect the network before owner reconciliation."}),
            ));
        }
    };
    sqlx::query("UPDATE publications SET state=$2,scheduled_at=CASE WHEN $2='queued' THEN now() ELSE scheduled_at END,last_error=NULL,updated_at=now() WHERE id=$1").bind(id).bind(target).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO publication_attempts(id,publication_id,state,provider,finished_at,receipt) VALUES($1,$2,'reconciled',$3,now(),$4)").bind(Uuid::new_v4()).bind(id).bind(&channel.app.provider).bind(receipt).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(data(
        crate::publications::get_publication(&state.db, id).await?,
    ))
}

/// Deliberate new upload only after an expired-session probe AND a later owner outcome attestation.
pub async fn reset_expired_session(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    auth.owner_session()?;
    let channel: Uuid = sqlx::query_scalar("SELECT channel_id FROM publications WHERE id=$1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    let mut tx = state.db.begin().await?;
    crate::schedules::lock_channel(&mut tx, channel).await?;
    let row = sqlx::query("SELECT p.brand_id,p.state,c.provider FROM publications p JOIN channels c ON c.id=p.channel_id WHERE p.id=$1 FOR UPDATE OF p").bind(id).fetch_one(&mut *tx).await?;
    auth.brand(row.get("brand_id"))?;
    if row.get::<String, _>("state") != "failed" || row.get::<String, _>("provider") != "youtube" {
        return Err(ApiError::conflict(
            "First reconcile the expired upload as not published after inspecting YouTube Studio",
        ));
    }
    let cp = checkpoint_on(&state, &mut tx, id)
        .await?
        .ok_or_else(|| ApiError::bad("No upload session to reset"))?;
    let attested: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM publication_attempts a JOIN provider_delivery_checkpoints cp ON cp.publication_id=a.publication_id WHERE a.publication_id=$1 AND a.state='failed' AND a.receipt->>'verification'='owner_attestation' AND a.receipt->'detail'->>'resolution'='not_published' AND a.finished_at>=cp.updated_at AND NOT EXISTS(SELECT 1 FROM publication_attempts later WHERE later.publication_id=a.publication_id AND later.started_at>a.finished_at AND later.state IN ('sending','uncertain','published')))").bind(id).fetch_one(&mut *tx).await?;
    if cp["expired"] != true || !attested {
        return Err(ApiError::conflict(
            "Reset requires an expired-session probe and a later recorded not-published owner attestation",
        ));
    }
    sqlx::query("DELETE FROM provider_delivery_checkpoints WHERE publication_id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO audit_events(id,owner_id,action,subject_id,detail) VALUES($1,$2,'reset_expired_upload',$3,$4)").bind(Uuid::new_v4()).bind(auth.owner_id).bind(id).bind(json!({"provider":"youtube"})).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(data(
        json!({"state":"failed","session_reset":true,"next_action":"Retry to start a new upload session"}),
    ))
}

pub async fn provider_status(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let row = sqlx::query("SELECT p.brand_id,p.state,c.provider FROM publications p JOIN channels c ON c.id=p.channel_id WHERE p.id=$1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    auth.brand(row.get("brand_id"))?;
    let provider: String = row.get("provider");
    let cp = if provider == "x" {
        let mut db = state.db.acquire().await?;
        x_checkpoint_on(&state, &mut db, id).await?
    } else {
        checkpoint(&state, id).await?
    };
    let timed_expired: bool = sqlx::query_scalar("SELECT COALESCE((SELECT expires_at<=clock_timestamp() FROM provider_delivery_checkpoints WHERE publication_id=$1),false)")
        .bind(id).fetch_one(&state.db).await?;
    let expired = timed_expired || cp.as_ref().is_some_and(|v| v["expired"] == true);
    let attested: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM publication_attempts a JOIN provider_delivery_checkpoints cp ON cp.publication_id=a.publication_id WHERE a.publication_id=$1 AND a.state='failed' AND a.receipt->>'verification'='owner_attestation' AND a.receipt->'detail'->>'resolution'='not_published' AND a.finished_at>=cp.updated_at AND NOT EXISTS(SELECT 1 FROM publication_attempts later WHERE later.publication_id=a.publication_id AND later.started_at>a.finished_at AND later.state IN ('sending','uncertain','published')))").bind(id).fetch_one(&state.db).await?;
    let current: String = row.get("state");
    let x_upload = if provider == "x" {
        cp.as_ref().filter(|cp| cp["stage"].as_str().is_some_and(|s| s.starts_with("x_"))).map(|cp| {
            let (attempt, checkpoint) = x_identity(cp)?;
            let public = x_public_boundary(cp);
            let reset_allowed = ["uncertain", "failed"].contains(&current.as_str())
                && (!public || (current == "failed" && x_attested(cp, attempt, checkpoint)));
                let reason = if current == "published" {
                    "A provider receipt is recorded; this delivery cannot be reset."
                } else if !["uncertain", "failed"].contains(&current.as_str()) {
                    "Delivery is still active or queued. Wait and reload status before recovery."
                } else if public && reset_allowed {
                    "The owner recorded a not-published observation for these IDs. Confirm the original sender stopped, then reset before an explicit retry."
                } else if public {
                "Stop the original sender, inspect X, and record a not-published resolution for these exact IDs before resetting. A provider request already accepted may still have an unknown outcome."
            } else {
                "Stop the original sender before resetting this upload; the reset does not retry or schedule a post."
            };
            Ok::<_, ApiError>(json!({"phase":if public {"post_started"} else {"upload"},"attempt_id":attempt,"checkpoint_id":checkpoint,"reset_allowed":reset_allowed,"reason":reason}))
        }).transpose()?
    } else {
        None
    };
    Ok(data(
        json!({"session_expired":expired,"reset_allowed":provider == "youtube" && expired && attested && current == "failed","reason":if expired && provider == "bluesky" {Some("The Bluesky processing window ended. Reconcile the saved video job before continuing.")} else if expired && provider == "youtube" {Some("Inspect YouTube Studio, record not-published reconciliation, then reset this expired upload before retrying.")}else{None},"x_upload":x_upload}),
    ))
}
