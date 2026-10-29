//! Owner-only provider configuration and browser-bound OAuth flows.
use crate::{
    auth::Auth,
    catalog, crypto,
    error::{ApiError, Result, data, required, uuid},
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Redirect, Response},
};
use beweft_providers::{self as providers, Adapter, AppConfig, Channel};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;
fn key(state: &AppState) -> Result<&[u8; 32]> {
    state.config.encryption_key.as_ref().ok_or_else(|| {
        ApiError::bad("Configure BEWEFT_ENCRYPTION_KEY before storing provider credentials")
    })
}
fn session_hash(headers: &HeaderMap) -> Result<String> {
    let token = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| {
            s.split(';')
                .find_map(|p| p.trim().strip_prefix("beweft_session="))
        })
        .ok_or_else(ApiError::unauthorized)?;
    Ok(crypto::digest(token))
}
fn optional_reconnect_channel(body: &Value) -> Result<Option<Uuid>> {
    match body.get("reconnect_channel_id") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => value
            .parse()
            .map(Some)
            .map_err(|_| ApiError::bad("reconnect_channel_id must be a UUID or null")),
        _ => Err(ApiError::bad("reconnect_channel_id must be a UUID or null")),
    }
}
fn provider_error(e: providers::ProviderError) -> ApiError {
    ApiError::bad(format!(
        "{}{}",
        e.message,
        e.status.map(|s| format!(" (HTTP {s})")).unwrap_or_default()
    ))
}

// Brand archival takes this same lock before channel locks. Holding it during
// connection storage prevents a new active channel appearing after archival.
async fn lock_active_brand(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    brand: Uuid,
) -> Result<()> {
    sqlx::query("SET LOCAL lock_timeout = '5s'")
        .execute(&mut **tx)
        .await?;
    crate::brand_admin::lock(tx, brand).await?;
    check_active_brand(tx, brand).await
}

async fn check_active_brand(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    brand: Uuid,
) -> Result<()> {
    let archived: bool =
        sqlx::query_scalar("SELECT archived_at IS NOT NULL FROM brands WHERE id=$1")
            .bind(brand)
            .fetch_one(&mut **tx)
            .await?;
    if archived {
        return Err(ApiError::conflict(
            "This brand is archived. Restore it before connecting a channel.",
        ));
    }
    Ok(())
}

mod apps;
mod credentials;
mod flows;
pub use apps::*;
use credentials::store_channel;
pub use credentials::*;
pub use flows::*;
