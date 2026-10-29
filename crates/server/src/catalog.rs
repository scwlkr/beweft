use crate::{
    auth::Auth,
    error::{ApiError, Result, data, required, uuid},
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
#[derive(Deserialize, Default)]
pub struct Filter {
    pub brand_id: Option<Uuid>,
    pub channel_id: Option<Uuid>,
}
pub async fn create_brand(
    State(state): State<AppState>,
    auth: Auth,
    Json(input): Json<crate::contracts::CreateBrandRequest>,
) -> Result<Json<Value>> {
    let body = serde_json::to_value(input).map_err(ApiError::internal)?;
    auth.scope("admin")?;
    if !auth.brand_ids.is_empty() {
        return Err(ApiError::forbidden());
    }
    let name = required(&body, "name")?;
    if name.len() > 100 {
        return Err(ApiError::bad("Brand name must be at most 100 bytes"));
    }
    let color = body["color"].as_str().unwrap_or("#8b5cf6");
    if color.len() != 7
        || !color.starts_with('#')
        || !color[1..].chars().all(|c| c.is_ascii_hexdigit())
    {
        return Err(ApiError::bad("color must be a six-digit hex color"));
    }
    let row: Value = sqlx::query_scalar(
        "INSERT INTO brands(id,name,color) VALUES($1,$2,$3) RETURNING to_jsonb(brands.*)",
    )
    .bind(Uuid::new_v4())
    .bind(name)
    .bind(color)
    .fetch_one(&state.db)
    .await?;
    Ok(data(row))
}
pub fn manual_capabilities() -> Value {
    json!({"execution":"manual","verification":"owner_attestation","formats":["text","image","video"],"publishing":true,"analytics":false,"inbox":false})
}
pub(crate) fn decorate_channel(mut channel: Value) -> Value {
    // Existing direct channels retain their granted scopes in stored JSON. Use
    // the installed Bluesky adapter's format description without changing those
    // permissions or requiring accounts to reconnect after an upgrade.
    if channel["provider"] == "bluesky"
        && channel["capabilities"]["execution"] == "direct"
        && let Some(capability) = beweft_providers::capabilities()
            .into_iter()
            .find(|c| c.id == "bluesky")
    {
        channel["capabilities"]["formats"] = json!(capability.formats);
        channel["capabilities"]["reason"] = json!(capability.reason);
    }
    crate::manual_channels::decorate(channel)
}
pub async fn channels(
    State(state): State<AppState>,
    auth: Auth,
    Query(filter): Query<Filter>,
) -> Result<Json<Value>> {
    if let Some(id) = filter.brand_id {
        auth.brand(id)?;
    }
    let rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(c)-'encrypted_credentials' FROM channels c WHERE ($1::uuid IS NULL OR brand_id=$1) AND (cardinality($2::uuid[])=0 OR brand_id=ANY($2)) ORDER BY created_at").bind(filter.brand_id).bind(&auth.brand_ids).fetch_all(&state.db).await?;
    Ok(data(
        rows.into_iter().map(decorate_channel).collect::<Vec<_>>(),
    ))
}
pub async fn create_channel(
    State(state): State<AppState>,
    auth: Auth,
    Json(input): Json<crate::contracts::CreateChannelRequest>,
) -> Result<Json<Value>> {
    let body = serde_json::to_value(input).map_err(ApiError::internal)?;
    auth.scope("admin")?;
    let brand = uuid(&body, "brand_id")?;
    auth.brand(brand)?;
    let name = required(&body, "name")?;
    if name.len() > 100 {
        return Err(ApiError::bad("Channel name must be at most 100 bytes"));
    }
    let provider = required(&body, "provider")?;
    if provider != "manual" && !(provider == "fixture" && state.config.fixture_enabled) {
        return Err(ApiError::bad(
            "Only manual channels are available; fixtures require server opt-in",
        ));
    }
    let timezone = body["timezone"].as_str().unwrap_or("UTC");
    validate_timezone(timezone)?;
    let caps = if provider == "manual" {
        manual_capabilities()
    } else {
        json!({"execution":"fixture","verification":"fixture_only","formats":["text","image","video"],"publishing":false,"analytics":false,"inbox":false})
    };
    let target = if provider == "manual" {
        let target = body["target_platform"].as_str().unwrap_or("generic");
        crate::manual_channels::guidance(target)?;
        Some(target)
    } else {
        if body.get("target_platform").is_some() {
            return Err(ApiError::bad("target_platform is for manual channels only"));
        }
        None
    };
    let mut tx = state.db.begin().await?;
    crate::brand_admin::lock(&mut tx, brand).await?;
    crate::brand_admin::active(&mut tx, brand).await?;
    let row:Value=sqlx::query_scalar("INSERT INTO channels(id,brand_id,name,provider,timezone,capabilities,target_platform) VALUES($1,$2,$3,$4,$5,$6,$7) RETURNING to_jsonb(channels.*)-'encrypted_credentials'").bind(Uuid::new_v4()).bind(brand).bind(name).bind(provider).bind(timezone).bind(caps).bind(target).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok(data(decorate_channel(row)))
}
pub async fn update_channel(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(input): Json<crate::contracts::UpdateChannelRequest>,
) -> Result<Json<Value>> {
    let body = serde_json::to_value(input).map_err(ApiError::internal)?;
    auth.scope("admin")?;
    let brand = channel_brand(&state, id).await?;
    auth.brand(brand)?;
    let name = body.get("name").and_then(Value::as_str);
    if name.is_some_and(|s| s.trim().is_empty() || s.len() > 100) {
        return Err(ApiError::bad("Invalid channel name"));
    }
    let timezone = body.get("timezone").and_then(Value::as_str);
    if let Some(t) = timezone {
        validate_timezone(t)?;
    }
    let mut tx = state.db.begin().await?;
    crate::schedules::lock_channel(&mut tx, id).await?;
    if body["paused"].as_bool() == Some(false) {
        crate::brand_admin::active(&mut tx, brand).await?;
    }
    let target = body["target_platform"].as_str();
    if let Some(target) = target {
        crate::manual_channels::guidance(target)?;
        let provider: String = sqlx::query_scalar("SELECT provider FROM channels WHERE id=$1")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
        if provider != "manual" {
            return Err(ApiError::bad("target_platform is for manual channels only"));
        }
    }

    let row:Value=sqlx::query_scalar("UPDATE channels SET name=COALESCE($2,name),paused=COALESCE($3,paused),timezone=COALESCE($4,timezone),target_platform=COALESCE($5,target_platform) WHERE id=$1 RETURNING to_jsonb(channels.*)-'encrypted_credentials'").bind(id).bind(name).bind(body.get("paused").and_then(Value::as_bool)).bind(timezone).bind(target).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok(data(decorate_channel(row)))
}
pub fn validate_timezone(t: &str) -> Result<chrono_tz::Tz> {
    t.parse()
        .map_err(|_| ApiError::bad("Unknown IANA time zone"))
}
pub async fn channel_brand(state: &AppState, id: Uuid) -> Result<Uuid> {
    Ok(
        sqlx::query_scalar("SELECT brand_id FROM channels WHERE id=$1")
            .bind(id)
            .fetch_one(&state.db)
            .await?,
    )
}
