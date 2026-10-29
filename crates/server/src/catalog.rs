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
pub async fn brands(State(state): State<AppState>, auth: Auth) -> Result<Json<Value>> {
    let rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(b) FROM brands b WHERE cardinality($1::uuid[])=0 OR id=ANY($1) ORDER BY created_at").bind(&auth.brand_ids).fetch_all(&state.db).await?;
    Ok(data(rows))
}
pub async fn create_brand(
    State(state): State<AppState>,
    auth: Auth,
    Json(body): Json<Value>,
) -> Result<Json<Value>> {
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
pub async fn providers(State(state): State<AppState>, _auth: Auth) -> Json<Value> {
    let mut rows = vec![
        json!({"id":"manual","name":"Manual handoff","implementation":"available","configuration":"ready","execution":"manual","verification":"owner_attestation","formats":["text","image","video"],"reason":"Prepare content and confirm publication yourself."}),
    ];
    for (id, name) in [
        ("x", "X"),
        ("youtube", "YouTube"),
        ("facebook", "Facebook"),
        ("instagram", "Instagram"),
        ("threads", "Threads"),
        ("tiktok", "TikTok"),
        ("bluesky", "Bluesky"),
        ("mastodon", "Mastodon"),
        ("linkedin", "LinkedIn"),
        ("pinterest", "Pinterest"),
    ] {
        rows.push(json!({"id":id,"name":name,"implementation":"unavailable","configuration":"not_configured","execution":"unavailable","verification":"unverified","formats":[],"reason":"Direct provider adapter is not installed in this foundation. Use a manual channel."}));
    }
    if state.config.fixture_enabled {
        rows.push(json!({"id":"fixture","name":"Isolated fixture","implementation":"available","configuration":"ready","execution":"fixture","verification":"fixture_only","formats":["text","image","video"],"reason":"Development fixture; never sends to a social network."}));
    }
    data(rows)
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
    Ok(data(rows))
}
pub async fn create_channel(
    State(state): State<AppState>,
    auth: Auth,
    Json(body): Json<Value>,
) -> Result<Json<Value>> {
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
    let row:Value=sqlx::query_scalar("INSERT INTO channels(id,brand_id,name,provider,timezone,capabilities) VALUES($1,$2,$3,$4,$5,$6) RETURNING to_jsonb(channels.*)-'encrypted_credentials'").bind(Uuid::new_v4()).bind(brand).bind(name).bind(provider).bind(timezone).bind(caps).fetch_one(&state.db).await?;
    Ok(data(row))
}
pub async fn update_channel(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(body): Json<Value>,
) -> Result<Json<Value>> {
    auth.scope("admin")?;
    auth.brand(channel_brand(&state, id).await?)?;
    let name = body.get("name").and_then(Value::as_str);
    if name.is_some_and(|s| s.trim().is_empty() || s.len() > 100) {
        return Err(ApiError::bad("Invalid channel name"));
    }
    let timezone = body.get("timezone").and_then(Value::as_str);
    if let Some(t) = timezone {
        validate_timezone(t)?;
    }
    let row:Value=sqlx::query_scalar("UPDATE channels SET name=COALESCE($2,name),paused=COALESCE($3,paused),timezone=COALESCE($4,timezone) WHERE id=$1 RETURNING to_jsonb(channels.*)-'encrypted_credentials'").bind(id).bind(name).bind(body.get("paused").and_then(Value::as_bool)).bind(timezone).fetch_one(&state.db).await?;
    Ok(data(row))
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
pub async fn ideas(
    State(state): State<AppState>,
    auth: Auth,
    Query(filter): Query<Filter>,
) -> Result<Json<Value>> {
    if let Some(id) = filter.brand_id {
        auth.brand(id)?;
    }
    let rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(i) FROM ideas i WHERE ($1::uuid IS NULL OR brand_id=$1) AND (cardinality($2::uuid[])=0 OR brand_id=ANY($2)) ORDER BY updated_at DESC").bind(filter.brand_id).bind(&auth.brand_ids).fetch_all(&state.db).await?;
    Ok(data(rows))
}
pub async fn create_idea(
    State(state): State<AppState>,
    auth: Auth,
    Json(body): Json<Value>,
) -> Result<Json<Value>> {
    auth.scope("drafts:write")?;
    let brand = uuid(&body, "brand_id")?;
    auth.brand(brand)?;
    let row:Value=sqlx::query_scalar("INSERT INTO ideas(id,brand_id,title,text,color) VALUES($1,$2,$3,$4,$5) RETURNING to_jsonb(ideas.*)").bind(Uuid::new_v4()).bind(brand).bind(required(&body,"title")?).bind(body["text"].as_str().unwrap_or("")).bind(body["color"].as_str().unwrap_or("lavender")).fetch_one(&state.db).await?;
    Ok(data(row))
}
pub async fn update_idea(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(body): Json<Value>,
) -> Result<Json<Value>> {
    auth.scope("drafts:write")?;
    let brand: Uuid = sqlx::query_scalar("SELECT brand_id FROM ideas WHERE id=$1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    auth.brand(brand)?;
    if body.get("title").is_some() {
        required(&body, "title")?;
    }
    let row:Value=sqlx::query_scalar("UPDATE ideas SET title=COALESCE($2,title),text=COALESCE($3,text),color=COALESCE($4,color),updated_at=now() WHERE id=$1 RETURNING to_jsonb(ideas.*)").bind(id).bind(body["title"].as_str()).bind(body["text"].as_str()).bind(body["color"].as_str()).fetch_one(&state.db).await?;
    Ok(data(row))
}
pub async fn delete_idea(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    auth.scope("drafts:write")?;
    let brand: Uuid = sqlx::query_scalar("SELECT brand_id FROM ideas WHERE id=$1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    auth.brand(brand)?;
    sqlx::query("DELETE FROM ideas WHERE id=$1")
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(data(json!({"ok":true})))
}
