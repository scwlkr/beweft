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
fn provider_error(e: providers::ProviderError) -> ApiError {
    ApiError::bad(format!(
        "{}{}",
        e.message,
        e.status.map(|s| format!(" (HTTP {s})")).unwrap_or_default()
    ))
}

mod apps;
mod credentials;
mod flows;
pub use apps::*;
use credentials::store_channel;
pub use credentials::*;
pub use flows::*;
