use crate::{
    auth::Auth,
    catalog::Filter,
    error::{Result, data},
    state::AppState,
};
use axum::{
    Json,
    extract::{Query, State},
};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
pub async fn health(State(state): State<AppState>) -> Result<Json<Value>> {
    sqlx::query("SELECT 1").execute(&state.db).await?;
    Ok(Json(json!({"status":"ok"})))
}
pub async fn jobs(State(state): State<AppState>, auth: Auth) -> Result<Json<Value>> {
    let last: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT max(last_seen) FROM worker_heartbeats")
            .fetch_one(&state.db)
            .await?;
    let counts:Value=sqlx::query_scalar("SELECT jsonb_build_object('queued',count(*) FILTER(WHERE state='queued'),'uncertain',count(*) FILTER(WHERE state='uncertain'),'manual_required',count(*) FILTER(WHERE state='manual_required'),'oldest_due',min(scheduled_at) FILTER(WHERE state='queued' AND scheduled_at<=now())) FROM publications WHERE cardinality($1::uuid[])=0 OR brand_id=ANY($1)").bind(&auth.brand_ids).fetch_one(&state.db).await?;
    let mut result = counts;
    result["worker_last_seen"] = json!(last);
    result["worker_healthy"] =
        json!(last.is_some_and(|t| t > Utc::now() - chrono::Duration::seconds(30)));
    Ok(data(result))
}
pub async fn analytics(
    State(state): State<AppState>,
    auth: Auth,
    Query(filter): Query<Filter>,
) -> Result<Json<Value>> {
    if let Some(brand) = filter.brand_id {
        auth.brand(brand)?;
    }
    let summary:Value=sqlx::query_scalar("SELECT jsonb_build_object('scheduled',count(*) FILTER(WHERE state IN ('queued','sending')),'published',count(*) FILTER(WHERE state='published'),'attention',count(*) FILTER(WHERE state IN ('uncertain','failed','manual_required'))) FROM publications WHERE ($1::uuid IS NULL OR brand_id=$1) AND (cardinality($2::uuid[])=0 OR brand_id=ANY($2))").bind(filter.brand_id).bind(&auth.brand_ids).fetch_one(&state.db).await?;
    Ok(data(
        json!({"availability":"unavailable","reason":"No connected provider currently supplies performance metrics. These summary counts describe local workflow only.","summary":summary,"metrics":[]}),
    ))
}
pub async fn inbox(auth: Auth, Query(filter): Query<Filter>) -> Result<Json<Value>> {
    if let Some(brand) = filter.brand_id {
        auth.brand(brand)?;
    }
    Ok(data(
        json!({"availability":"unavailable","reason":"Inbox access requires a supported provider integration and granted account permissions.","items":[]}),
    ))
}
