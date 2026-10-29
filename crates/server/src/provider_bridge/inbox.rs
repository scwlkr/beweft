use super::*;
use sqlx::Acquire;
pub async fn inbox(
    State(state): State<AppState>,
    auth: Auth,
    Query(filter): Query<catalog::Filter>,
) -> Result<Json<Value>> {
    if let Some(brand) = filter.brand_id {
        auth.brand(brand)?;
    }
    let items:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',i.id,'channel_id',i.channel_id,'channel_name',c.name,'provider',c.provider,'item',i.item,'read_at',i.read_at,'synced_at',i.synced_at,'can_reply',(c.connection_status='connected' AND COALESCE((c.capabilities->>'reply')::boolean,false) AND COALESCE(c.capabilities->'permissions'->'actions'->'reply'->>'status','unknown') <> 'missing')) FROM provider_inbox i JOIN channels c ON c.id=i.channel_id WHERE ($1::uuid IS NULL OR c.brand_id=$1) AND (cardinality($2::uuid[])=0 OR c.brand_id=ANY($2)) ORDER BY i.synced_at DESC LIMIT 200").bind(filter.brand_id).bind(&auth.brand_ids).fetch_all(&state.db).await?;
    Ok(data(
        json!({"availability":if items.is_empty(){"unavailable"}else{"partial"},"reason":"Recent comments and mentions from explicit channel syncs. This is not a complete DM inbox; unsupported conversations remain on the network.","items":items}),
    ))
}
pub async fn mark_read(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    auth.scope("publishing:write")?;
    let brand:Uuid=sqlx::query_scalar("SELECT c.brand_id FROM provider_inbox i JOIN channels c ON c.id=i.channel_id WHERE i.id=$1").bind(id).fetch_one(&state.db).await?;
    auth.brand(brand)?;
    sqlx::query("UPDATE provider_inbox SET read_at=now() WHERE id=$1")
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(data(json!({"ok":true})))
}
pub async fn reply(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(body): Json<Value>,
) -> Result<Json<Value>> {
    auth.scope("publishing:write")?;
    let text = required(&body, "text")?;
    let request_id: Uuid = required(&body, "request_id")?.parse().map_err(|_| {
        ApiError::bad("request_id must be a client-generated UUID reused on retries")
    })?;
    if text.len() > 10000 {
        return Err(ApiError::bad("Reply is too long"));
    }
    // A reply retains one DB session until outbound execution and completion
    // finish. Share sync admission so at least one pool slot stays available.
    let _permit = state
        .provider_slots
        .try_acquire()
        .map_err(|_| ApiError::conflict("Provider capacity is busy; try again shortly"))?;
    let mut db = state.db.acquire().await?;
    // A canceled acquisition may have installed the session lock already.
    // Closing rather than returning the connection guarantees its release.
    db.close_on_drop();
    let row=sqlx::query("SELECT i.channel_id,i.provider_item_id,i.item,c.brand_id FROM provider_inbox i JOIN channels c ON c.id=i.channel_id WHERE i.id=$1").bind(id).fetch_one(&mut *db).await?;
    auth.brand(row.get("brand_id"))?;
    let acquired: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock(hashtextextended($1,0))")
        .bind(super::reply_execution_key(id))
        .fetch_one(&mut *db)
        .await?;
    if !acquired {
        return Err(ApiError::conflict_code(
            "reply_execution_active",
            "A reply sender is still active. Wait for it to stop, then reload reply history before trying again.",
        ));
    }
    let channel_id: Uuid = row.get("channel_id");
    let mut tx = db.begin().await?;
    crate::schedules::lock_channel(&mut tx, channel_id).await?;
    if let Some(value) =
        super::reply_claim::check_history(&mut tx, &auth, id, request_id, &body).await?
    {
        tx.commit().await?;
        return Ok(data(value));
    }
    tx.commit().await?;
    // Commit any token rotation before later claim/adaptor errors can roll it back.
    let mut refresh = db.begin().await?;
    let refreshed = connections::load_channel_tx(&state, &mut refresh, channel_id, None).await;
    refresh.commit().await?;
    refreshed?;
    let mut tx = db.begin().await?;
    crate::schedules::lock_channel(&mut tx, channel_id).await?;
    if let Some(value) =
        super::reply_claim::check_history(&mut tx, &auth, id, request_id, &body).await?
    {
        tx.commit().await?;
        return Ok(data(value));
    }
    let c = connections::load_stored_channel_tx(&state, &mut tx, channel_id).await?;
    let adapter = connections::adapter_for(&c.app).await?;
    let item: providers::InboxItem =
        serde_json::from_value(row.get("item")).map_err(ApiError::internal)?;
    let started = providers::SuspendInstant::now().ok_or_else(|| {
        ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "provider_clock_unavailable",
            "The reply could not start because its elapsed-time clock is unavailable.".into(),
        )
    })?;
    let remaining:Option<f64>=sqlx::query_scalar("INSERT INTO provider_replies(id,inbox_id,owner_id,text,state,created_at,updated_at,delivery_deadline) VALUES($1,$2,$3,$4,'sending',clock_timestamp(),clock_timestamp(),clock_timestamp()+interval '90 seconds') ON CONFLICT(id) DO NOTHING RETURNING EXTRACT(EPOCH FROM (delivery_deadline-clock_timestamp()))::double precision").bind(request_id).bind(id).bind(auth.owner_id).bind(&text).fetch_optional(&mut *tx).await?;
    let remaining = remaining.ok_or_else(|| {
        ApiError::conflict("Reply request already exists; inspect its original result")
    })?;
    // Do not compare a PostgreSQL timestamp to the API host's wall clock.
    // Account for query and commit time, and leave a one-second safety margin.
    let budget = if remaining.is_finite() && remaining > 1.0 {
        std::time::Duration::from_secs_f64((remaining - 1.0).min(90.0))
    } else {
        std::time::Duration::ZERO
    };
    let deadline = started.deadline(budget).ok_or_else(|| {
        ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "provider_clock_unavailable",
            "The reply could not start because its elapsed-time clock is unavailable.".into(),
        )
    })?;
    let adapter = adapter.with_deadline(deadline);
    tx.commit().await?;
    let (status, receipt, error) = match adapter
        .reply(&c, &item, &text, &request_id.to_string())
        .await
    {
        Ok(Step::Published {
            external_id,
            external_url,
            receipt,
        }) => (
            "published",
            Some(json!({"external_id":external_id,"external_url":external_url,"provider":receipt})),
            None,
        ),
        Ok(_) => (
            "uncertain",
            None,
            Some("Provider reply requires reconciliation".to_owned()),
        ),
        Err(e) => (
            if matches!(e.kind, FailureKind::Uncertain | FailureKind::Unavailable) {
                "uncertain"
            } else {
                "failed"
            },
            None,
            Some(e.message),
        ),
    };
    let result:Option<Value>=sqlx::query_scalar("UPDATE provider_replies SET state=$2,receipt=$3,error=$4,updated_at=clock_timestamp() WHERE id=$1 AND state='sending' AND delivery_deadline>clock_timestamp() RETURNING to_jsonb(provider_replies.*)-'owner_id'-'history_order'-'delivery_deadline'").bind(request_id).bind(status).bind(receipt).bind(error).fetch_optional(&mut *db).await?;
    let result = match result {
        Some(value) => value,
        None => {
            // A response arriving after its deadline cannot install a receipt
            // or override an owner's later resolution. Quarantine it instead.
            let expired:Option<Value>=sqlx::query_scalar("UPDATE provider_replies SET state='uncertain',error='Reply deadline elapsed; inspect the network before resolving the outcome',updated_at=clock_timestamp() WHERE id=$1 AND state='sending' AND delivery_deadline<=clock_timestamp() RETURNING to_jsonb(provider_replies.*)-'owner_id'-'history_order'-'delivery_deadline'")
                .bind(request_id).fetch_optional(&mut *db).await?;
            match expired {
                Some(value) => value,
                // The row may already have been projected/quarantined as
                // uncertain. Record that this executor ended even then, so an
                // observation made while it was running can never authorize a
                // later resolution. Preserve any stronger owner outcome.
                None => sqlx::query_scalar("UPDATE provider_replies SET updated_at=clock_timestamp() WHERE id=$1 AND owner_id=$2 RETURNING to_jsonb(provider_replies.*)-'owner_id'-'history_order'-'delivery_deadline'")
                    .bind(request_id).bind(auth.owner_id).fetch_one(&mut *db).await?,
            }
        }
    };
    Ok(data(result))
}
