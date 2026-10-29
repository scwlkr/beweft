use crate::{
    auth::Auth,
    crypto,
    error::{ApiError, Result, data, required},
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, State},
};
use serde_json::{Value, json};
use uuid::Uuid;
pub async fn list(State(state): State<AppState>, auth: Auth) -> Result<Json<Value>> {
    auth.owner_session()?;
    let rows:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'name',name,'scopes',scopes,'brand_ids',brand_ids,'created_at',created_at) FROM api_tokens WHERE owner_id=$1 ORDER BY created_at DESC").bind(auth.owner_id).fetch_all(&state.db).await?;
    Ok(data(rows))
}
pub async fn create(
    State(state): State<AppState>,
    auth: Auth,
    Json(input): Json<crate::contracts::CreateTokenRequest>,
) -> Result<Json<Value>> {
    let body = serde_json::to_value(input).map_err(ApiError::internal)?;
    auth.owner_session()?;
    let name = required(&body, "name")?;
    let scopes: Vec<String> =
        serde_json::from_value(body.get("scopes").cloned().unwrap_or(json!(["read"])))
            .map_err(|_| ApiError::bad("scopes must be an array"))?;
    if scopes.is_empty()
        || scopes
            .iter()
            .any(|s| !["read", "drafts:write", "publishing:write", "admin"].contains(&s.as_str()))
    {
        return Err(ApiError::bad("Unknown or empty token scopes"));
    }
    let brands: Vec<Uuid> =
        serde_json::from_value(body.get("brand_ids").cloned().unwrap_or(json!([])))
            .map_err(|_| ApiError::bad("brand_ids must be UUIDs"))?;
    let id = Uuid::new_v4();
    let token = format!("bw_{}", crypto::random_secret());
    sqlx::query("INSERT INTO api_tokens(id,owner_id,name,token_hash,scopes,brand_ids) VALUES($1,$2,$3,$4,$5,$6)").bind(id).bind(auth.owner_id).bind(&name).bind(crypto::digest(&token)).bind(json!(scopes)).bind(json!(brands)).execute(&state.db).await?;
    Ok(data(
        json!({"id":id,"token":token,"name":name,"scopes":scopes,"brand_ids":brands}),
    ))
}
pub async fn delete(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    auth.owner_session()?;
    let result = sqlx::query("DELETE FROM api_tokens WHERE id=$1 AND owner_id=$2")
        .bind(id)
        .bind(auth.owner_id)
        .execute(&state.db)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::missing());
    }
    Ok(data(json!({"ok":true})))
}
