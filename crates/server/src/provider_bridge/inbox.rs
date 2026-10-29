use super::*;
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
    let row=sqlx::query("SELECT i.channel_id,i.provider_item_id,i.item,c.brand_id FROM provider_inbox i JOIN channels c ON c.id=i.channel_id WHERE i.id=$1").bind(id).fetch_one(&state.db).await?;
    auth.brand(row.get("brand_id"))?;
    let channel_id: Uuid = row.get("channel_id");
    let mut tx = state.db.begin().await?;
    crate::schedules::lock_channel(&mut tx, channel_id).await?;
    if let Some(value) =
        super::reply_claim::check_history(&mut tx, &auth, id, request_id, &body).await?
    {
        tx.commit().await?;
        return Ok(data(value));
    }
    tx.commit().await?;
    // Commit any token rotation before later claim/adaptor errors can roll it back.
    connections::load_channel(&state, channel_id).await?;
    let mut tx = state.db.begin().await?;
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
    let deadline:Option<chrono::DateTime<chrono::Utc>>=sqlx::query_scalar("INSERT INTO provider_replies(id,inbox_id,owner_id,text,state,created_at,updated_at,delivery_deadline) VALUES($1,$2,$3,$4,'sending',clock_timestamp(),clock_timestamp(),clock_timestamp()+interval '90 seconds') ON CONFLICT(id) DO NOTHING RETURNING delivery_deadline").bind(request_id).bind(id).bind(auth.owner_id).bind(&text).fetch_optional(&mut *tx).await?;
    let deadline = deadline.ok_or_else(|| {
        ApiError::conflict("Reply request already exists; inspect its original result")
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
    let result:Option<Value>=sqlx::query_scalar("UPDATE provider_replies SET state=$2,receipt=$3,error=$4,updated_at=now() WHERE id=$1 AND state='sending' RETURNING to_jsonb(provider_replies.*)-'owner_id'-'history_order'-'delivery_deadline'").bind(request_id).bind(status).bind(receipt).bind(error).fetch_optional(&state.db).await?;
    let result =
        match result {
            Some(value) => value,
            None => sqlx::query_scalar(
                "SELECT to_jsonb(r)-'owner_id'-'history_order'-'delivery_deadline' FROM provider_replies r WHERE id=$1 AND owner_id=$2",
            )
            .bind(request_id)
            .bind(auth.owner_id)
            .fetch_one(&state.db)
            .await?,
        };
    Ok(data(result))
}
