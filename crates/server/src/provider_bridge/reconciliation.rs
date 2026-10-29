use super::delivery::save_checkpoint_tx;
use super::preparation::{checkpoint, checkpoint_on, load_publication_on};
use super::*;

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
    let row:Value=sqlx::query_scalar("UPDATE publications SET state=$2,scheduled_at=CASE WHEN $2='queued' THEN now() ELSE scheduled_at END,last_error=NULL,updated_at=now() WHERE id=$1 RETURNING to_jsonb(publications.*)").bind(id).bind(target).fetch_one(&mut *tx).await?;
    sqlx::query("INSERT INTO publication_attempts(id,publication_id,state,provider,finished_at,receipt) VALUES($1,$2,'reconciled',$3,now(),$4)").bind(Uuid::new_v4()).bind(id).bind(&channel.app.provider).bind(receipt).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(data(row))
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
    let row = sqlx::query("SELECT brand_id,state FROM publications WHERE id=$1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    auth.brand(row.get("brand_id"))?;
    let cp = checkpoint(&state, id).await?;
    let expired = cp.as_ref().is_some_and(|v| v["expired"] == true);
    let attested: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM publication_attempts a JOIN provider_delivery_checkpoints cp ON cp.publication_id=a.publication_id WHERE a.publication_id=$1 AND a.state='failed' AND a.receipt->>'verification'='owner_attestation' AND a.receipt->'detail'->>'resolution'='not_published' AND a.finished_at>=cp.updated_at AND NOT EXISTS(SELECT 1 FROM publication_attempts later WHERE later.publication_id=a.publication_id AND later.started_at>a.finished_at AND later.state IN ('sending','uncertain','published')))").bind(id).fetch_one(&state.db).await?;
    Ok(data(
        json!({"session_expired":expired,"reset_allowed":expired && attested && row.get::<String,_>("state") == "failed","reason":if expired {Some("Inspect YouTube Studio, record not-published reconciliation, then reset this expired upload before retrying.")}else{None}}),
    ))
}
