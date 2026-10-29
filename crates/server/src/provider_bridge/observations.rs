use super::*;
use sqlx::Acquire;
pub async fn sync(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(body): Json<Value>,
) -> Result<Json<Value>> {
    auth.scope("publishing:write")?;
    auth.brand(catalog::channel_brand(&state, id).await?)?;
    // Keep the nonblocking sync lock across the separately committed refresh and
    // the downstream sync, using one pool connection throughout. A cancelled
    // query may have acquired a session lock before its result was received, so
    // close this connection on every exit instead of returning a lock to the pool.
    let mut db = state.db.acquire().await?;
    db.close_on_drop();
    let lock: i64 = id.as_u128() as i64;
    let acquired: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
        .bind(lock)
        .fetch_one(&mut *db)
        .await?;
    if !acquired {
        return Err(ApiError::conflict("This channel is already syncing"));
    }
    let mut refresh = db.begin().await?;
    let refreshed = connections::load_channel_tx(&state, &mut refresh, id, None).await;
    refresh.commit().await?;
    refreshed?;

    // Reacquire the channel row for the full sync so disconnect cannot revoke
    // a grant in use; downstream failures cannot roll back a rotated token.
    let mut guard = db.begin().await?;
    let c = connections::load_stored_channel_tx(&state, &mut guard, id).await?;
    if c.app.provider == "x" && body["allow_metered"].as_bool() != Some(true) {
        return Err(ApiError::bad(
            "X reads can incur charges. Set allow_metered=true only after approving provider costs.",
        ));
    }
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        sync_inner(&state, &mut guard, id, &c),
    )
    .await
    .map_err(|_| ApiError::conflict("Provider sync timed out; try again"))?;
    guard.commit().await?;
    result
}
async fn sync_inner(
    _state: &AppState,
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: Uuid,
    c: &providers::Channel,
) -> Result<Json<Value>> {
    let adapter = connections::adapter_for(&c.app).await?;
    let cap = providers::capabilities()
        .into_iter()
        .find(|v| v.id == c.app.provider)
        .unwrap();
    let recent=sqlx::query("SELECT id,external_id FROM publications WHERE channel_id=$1 AND state='published' AND external_id IS NOT NULL ORDER BY updated_at DESC LIMIT 10").bind(id).fetch_all(&mut **tx).await?;
    let mut errors = vec![];
    let mut metric_count = 0;
    let mut item_count = 0;
    if cap.analytics {
        for p in &recent {
            let external: String = p.get("external_id");
            match adapter.metrics(c, &external).await {
                Ok(metrics) => {
                    if !metrics.is_empty() {
                        sqlx::query(
                            "INSERT INTO provider_metrics(publication_id,metrics) VALUES($1,$2)",
                        )
                        .bind(p.get::<Uuid, _>("id"))
                        .bind(json!(metrics))
                        .execute(&mut **tx)
                        .await?;
                        metric_count += metrics.len();
                    }
                }
                Err(e) => {
                    errors.push(e.message);
                    break;
                }
            }
        }
    }
    if cap.inbox {
        let ids = recent
            .iter()
            .map(|r| r.get("external_id"))
            .collect::<Vec<String>>();
        match adapter.inbox(c, &ids).await {
            Ok(items) => {
                for item in items {
                    sqlx::query("INSERT INTO provider_inbox(id,channel_id,provider_item_id,item) VALUES($1,$2,$3,$4) ON CONFLICT(channel_id,provider_item_id) DO UPDATE SET item=EXCLUDED.item,synced_at=now()").bind(Uuid::new_v4()).bind(id).bind(&item.id).bind(json!(item)).execute(&mut **tx).await?;
                    item_count += 1;
                }
            }
            Err(e) => errors.push(e.message),
        }
    }
    let message = if errors.is_empty() {
        None
    } else {
        Some(errors.join(" "))
    };
    sqlx::query("INSERT INTO provider_syncs(channel_id,last_success_at,last_error) VALUES($1,CASE WHEN $2::text IS NULL THEN now() ELSE NULL END,$2) ON CONFLICT(channel_id) DO UPDATE SET last_attempt_at=now(),last_success_at=CASE WHEN EXCLUDED.last_error IS NULL THEN now() ELSE provider_syncs.last_success_at END,last_error=EXCLUDED.last_error").bind(id).bind(&message).execute(&mut **tx).await?;
    Ok(data(
        json!({"metrics_observed":metric_count,"inbox_items":item_count,"coverage":"partial_recent_page","errors":errors,"note":"Recent comments/mentions and up to ten local publications; this is not a complete network inbox or historical analytics import."}),
    ))
}
pub async fn analytics(
    State(state): State<AppState>,
    auth: Auth,
    Query(filter): Query<catalog::Filter>,
) -> Result<Json<Value>> {
    if let Some(brand) = filter.brand_id {
        auth.brand(brand)?;
    }
    let rows:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('publication_id',p.id,'channel_id',p.channel_id,'provider',c.provider,'text',p.payload->>'text','external_url',p.external_url,'observed_at',m.observed_at,'metrics',m.metrics) FROM publications p JOIN channels c ON c.id=p.channel_id JOIN LATERAL (SELECT * FROM provider_metrics WHERE publication_id=p.id ORDER BY observed_at DESC LIMIT 1) m ON true WHERE ($1::uuid IS NULL OR p.brand_id=$1) AND (cardinality($2::uuid[])=0 OR p.brand_id=ANY($2)) ORDER BY m.observed_at DESC LIMIT 200").bind(filter.brand_id).bind(&auth.brand_ids).fetch_all(&state.db).await?;
    let summary:Value=sqlx::query_scalar("SELECT jsonb_build_object('scheduled',count(*) FILTER(WHERE state='queued'),'published',count(*) FILTER(WHERE state='published'),'attention',count(*) FILTER(WHERE state IN ('failed','uncertain','manual_required'))) FROM publications WHERE ($1::uuid IS NULL OR brand_id=$1) AND (cardinality($2::uuid[])=0 OR brand_id=ANY($2))").bind(filter.brand_id).bind(&auth.brand_ids).fetch_one(&state.db).await?;
    Ok(data(
        json!({"availability":if rows.is_empty(){"unavailable"}else{"partial"},"reason":"Metrics are fetched on demand from supported connected channels. Missing observations are not zero. Summary counts describe local workflow only.","summary":summary,"metrics":rows}),
    ))
}
