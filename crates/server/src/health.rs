use crate::{
    auth::Auth,
    error::{Result, data},
    state::AppState,
};
use axum::{Json, extract::State};
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
