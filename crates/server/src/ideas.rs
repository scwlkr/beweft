use crate::{
    auth::Auth,
    contracts::{CreateIdeaRequest, IdeaStatus, UpdateIdeaRequest},
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
pub struct IdeaFilter {
    pub brand_id: Option<Uuid>,
    pub group_id: Option<Uuid>,
    pub status: Option<IdeaStatus>,
    pub tag: Option<String>,
}
pub async fn list(
    State(state): State<AppState>,
    auth: Auth,
    Query(filter): Query<IdeaFilter>,
) -> Result<Json<Value>> {
    if let Some(id) = filter.brand_id {
        auth.brand(id)?;
    }
    let status = filter.status.map(|s| {
        serde_json::to_value(s)
            .unwrap()
            .as_str()
            .unwrap()
            .to_string()
    });
    let rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(i) FROM ideas i WHERE ($1::uuid IS NULL OR brand_id=$1) AND (cardinality($2::uuid[])=0 OR brand_id=ANY($2)) AND ($3::uuid IS NULL OR group_id=$3) AND ($4::text IS NULL OR status=$4) AND ($5::text IS NULL OR tags ? $5) ORDER BY updated_at DESC").bind(filter.brand_id).bind(&auth.brand_ids).bind(filter.group_id).bind(status).bind(filter.tag).fetch_all(&state.db).await?;
    Ok(data(rows))
}
async fn validated(state: &AppState, brand: Uuid, body: &mut Value) -> Result<()> {
    if body.get("title").is_some() {
        let title = required(body, "title")?;
        if title.chars().count() > 300 {
            return Err(ApiError::bad("Idea title must be at most 300 characters"));
        }
    }
    if body["text"].as_str().is_some_and(|s| s.len() > 100_000) {
        return Err(ApiError::bad("Idea text is too long"));
    }
    if let Some(group) = body["group_id"].as_str() {
        let group: Uuid = group
            .parse()
            .map_err(|_| ApiError::bad("Invalid group_id"))?;
        let valid: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM idea_groups WHERE id=$1 AND brand_id=$2)",
        )
        .bind(group)
        .bind(brand)
        .fetch_one(&state.db)
        .await?;
        if !valid {
            return Err(ApiError::bad("Idea group must belong to the same brand"));
        }
    }
    if let Some(tags) = body.get("tags") {
        let tags: Vec<String> = serde_json::from_value(tags.clone())
            .map_err(|_| ApiError::bad("tags must be an array of strings"))?;
        if tags.len() > 20 {
            return Err(ApiError::bad("At most 20 tags are allowed"));
        }
        let mut cleaned = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for tag in tags {
            let tag = tag.trim();
            if tag.is_empty() || tag.chars().count() > 40 {
                return Err(ApiError::bad("Each tag must contain 1–40 characters"));
            }
            if seen.insert(tag.to_lowercase()) {
                cleaned.push(tag.to_string());
            }
        }
        body["tags"] = json!(cleaned);
    }
    Ok(())
}
pub async fn create(
    State(state): State<AppState>,
    auth: Auth,
    Json(input): Json<CreateIdeaRequest>,
) -> Result<Json<Value>> {
    auth.scope("drafts:write")?;
    let mut body = serde_json::to_value(input).map_err(ApiError::internal)?;
    let brand = uuid(&body, "brand_id")?;
    auth.brand(brand)?;
    validated(&state, brand, &mut body).await?;
    let group = body["group_id"]
        .as_str()
        .map(str::parse::<Uuid>)
        .transpose()
        .map_err(|_| ApiError::bad("Invalid group"))?;
    let row:Value=sqlx::query_scalar("INSERT INTO ideas(id,brand_id,title,text,color,status,group_id,tags) VALUES($1,$2,$3,$4,$5,$6,$7,$8) RETURNING to_jsonb(ideas.*)").bind(Uuid::new_v4()).bind(brand).bind(required(&body,"title")?).bind(body["text"].as_str().unwrap_or("")).bind(body["color"].as_str().unwrap_or("lavender")).bind(body["status"].as_str().unwrap_or("inbox")).bind(group).bind(body.get("tags").cloned().unwrap_or(json!([]))).fetch_one(&state.db).await?;
    Ok(data(row))
}
pub async fn update(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateIdeaRequest>,
) -> Result<Json<Value>> {
    auth.scope("drafts:write")?;
    let mut body = serde_json::to_value(input).map_err(ApiError::internal)?;
    let brand: Uuid = sqlx::query_scalar("SELECT brand_id FROM ideas WHERE id=$1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    auth.brand(brand)?;
    validated(&state, brand, &mut body).await?;
    let group = body["group_id"]
        .as_str()
        .map(str::parse::<Uuid>)
        .transpose()
        .map_err(|_| ApiError::bad("Invalid group"))?;
    let row:Value=sqlx::query_scalar("UPDATE ideas SET title=COALESCE($2,title),text=COALESCE($3,text),color=COALESCE($4,color),status=COALESCE($5,status),group_id=CASE WHEN $6 THEN $7 ELSE group_id END,tags=COALESCE($8,tags),updated_at=now() WHERE id=$1 RETURNING to_jsonb(ideas.*)").bind(id).bind(body["title"].as_str()).bind(body["text"].as_str()).bind(body["color"].as_str()).bind(body["status"].as_str()).bind(body.get("group_id").is_some()).bind(group).bind(body.get("tags")).fetch_one(&state.db).await?;
    Ok(data(row))
}
pub async fn delete(
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
