use crate::{
    auth::Auth,
    catalog::Filter,
    contracts::{CreateIdeaGroupRequest, UpdateIdeaGroupRequest},
    error::{ApiError, Result, data},
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde_json::{Value, json};
use uuid::Uuid;
fn validate(name: Option<&str>, color: Option<&str>) -> Result<()> {
    if name.is_some_and(|n| n.trim().is_empty() || n.chars().count() > 80)
        || color.is_some_and(|c| c.len() > 40)
    {
        return Err(ApiError::bad(
            "Group name must contain 1–80 characters and color at most 40 bytes",
        ));
    }
    Ok(())
}
pub async fn list(
    State(state): State<AppState>,
    auth: Auth,
    Query(filter): Query<Filter>,
) -> Result<Json<Value>> {
    if let Some(id) = filter.brand_id {
        auth.brand(id)?;
    }
    let rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(g) FROM idea_groups g WHERE ($1::uuid IS NULL OR brand_id=$1) AND (cardinality($2::uuid[])=0 OR brand_id=ANY($2)) ORDER BY position,created_at").bind(filter.brand_id).bind(&auth.brand_ids).fetch_all(&state.db).await?;
    Ok(data(rows))
}
pub async fn create(
    State(state): State<AppState>,
    auth: Auth,
    Json(body): Json<CreateIdeaGroupRequest>,
) -> Result<Json<Value>> {
    auth.scope("drafts:write")?;
    auth.brand(body.brand_id)?;
    validate(Some(&body.name), body.color.as_deref())?;
    let row:Value=sqlx::query_scalar("INSERT INTO idea_groups(id,brand_id,name,color,position) VALUES($1,$2,$3,$4,$5) RETURNING to_jsonb(idea_groups.*)").bind(Uuid::new_v4()).bind(body.brand_id).bind(body.name.trim()).bind(body.color.unwrap_or("lavender".into())).bind(body.position).fetch_one(&state.db).await?;
    Ok(data(row))
}
pub async fn update(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateIdeaGroupRequest>,
) -> Result<Json<Value>> {
    auth.scope("drafts:write")?;
    let brand: Uuid = sqlx::query_scalar("SELECT brand_id FROM idea_groups WHERE id=$1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    auth.brand(brand)?;
    validate(body.name.as_deref(), body.color.as_deref())?;
    let row:Value=sqlx::query_scalar("UPDATE idea_groups SET name=COALESCE($2,name),color=COALESCE($3,color),position=COALESCE($4,position) WHERE id=$1 RETURNING to_jsonb(idea_groups.*)").bind(id).bind(body.name.map(|s|s.trim().to_string())).bind(body.color).bind(body.position).fetch_one(&state.db).await?;
    Ok(data(row))
}
pub async fn delete(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    auth.scope("drafts:write")?;
    let brand: Uuid = sqlx::query_scalar("SELECT brand_id FROM idea_groups WHERE id=$1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    auth.brand(brand)?;
    sqlx::query("DELETE FROM idea_groups WHERE id=$1")
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(data(json!({"ok":true})))
}
