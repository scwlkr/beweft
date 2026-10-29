use super::*;

/// Reads never send replies or change their durable outcomes. A lost handler's stale
/// sending state is projected as uncertain so another browser can recover safely.
pub async fn replies(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let brand:Uuid=sqlx::query_scalar("SELECT c.brand_id FROM provider_inbox i JOIN channels c ON c.id=i.channel_id WHERE i.id=$1").bind(id).fetch_one(&state.db).await?;
    auth.brand(brand)?;
    let rows:Vec<Value>=sqlx::query_scalar("SELECT (to_jsonb(r)-'owner_id'-'history_order'-'delivery_deadline') || CASE WHEN state='sending' AND delivery_deadline<clock_timestamp() THEN jsonb_build_object('state','uncertain','error','Reply response was lost; inspect the network before resolving its outcome') ELSE '{}'::jsonb END FROM provider_replies r WHERE inbox_id=$1 AND owner_id=$2 ORDER BY history_order DESC LIMIT 100").bind(id).bind(auth.owner_id).fetch_all(&state.db).await?;
    Ok(data(json!(rows)))
}
pub async fn resolve_reply(
    State(state): State<AppState>,
    auth: Auth,
    Path((id, request_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<Value>,
) -> Result<Json<Value>> {
    auth.owner_session()?;
    let note = required(&body, "note")?;
    if note.len() > 2000 {
        return Err(ApiError::bad("Resolution note must be at most 2000 bytes"));
    }
    let outcome = match body["resolution"].as_str() {
        Some("published") => "published",
        Some("not_published") => "failed",
        _ => {
            return Err(ApiError::bad(
                "Choose published or not_published after inspecting the provider",
            ));
        }
    };
    if let Some(url) = body["external_url"].as_str() {
        let parsed = url::Url::parse(url).map_err(|_| ApiError::bad("Invalid external URL"))?;
        if !matches!(parsed.scheme(), "https" | "http")
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err(ApiError::bad(
                "External URL must use HTTP(S) without credentials",
            ));
        }
    }
    let context=sqlx::query("SELECT i.channel_id,c.brand_id FROM provider_inbox i JOIN channels c ON c.id=i.channel_id WHERE i.id=$1").bind(id).fetch_one(&state.db).await?;
    auth.brand(context.get("brand_id"))?;
    let mut tx = state.db.begin().await?;
    crate::schedules::lock_channel(&mut tx, context.get("channel_id")).await?;
    let row=sqlx::query("SELECT state,delivery_deadline<clock_timestamp() AS expired FROM provider_replies WHERE id=$1 AND inbox_id=$2 AND owner_id=$3 FOR UPDATE").bind(request_id).bind(id).bind(auth.owner_id).fetch_one(&mut *tx).await?;
    let status: String = row.get("state");
    if status != "uncertain" && !(status == "sending" && row.get::<bool, _>("expired")) {
        return Err(ApiError::conflict(
            "Only uncertain replies can be resolved; wait for an active reply to finish",
        ));
    }
    let receipt = json!({"verification":"owner_attestation","resolution":body["resolution"],"note":note,"external_url":body["external_url"]});
    let result:Value=sqlx::query_scalar("UPDATE provider_replies SET state=$2,receipt=$3,error=NULL,updated_at=now() WHERE id=$1 RETURNING to_jsonb(provider_replies.*)-'owner_id'-'history_order'-'delivery_deadline'").bind(request_id).bind(outcome).bind(&receipt).fetch_one(&mut *tx).await?;
    sqlx::query("INSERT INTO audit_events(id,owner_id,action,subject_id,detail) VALUES($1,$2,'resolve_reply',$3,$4)").bind(Uuid::new_v4()).bind(auth.owner_id).bind(request_id).bind(receipt).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(data(result))
}
