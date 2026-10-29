//! Durable connection between application actions and direct provider adapters.
use crate::{
    auth::Auth,
    catalog, connections, crypto,
    error::{ApiError, Result, data, required},
    state::AppState,
};
use axum::{
    Json,
    body::Body,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use beweft_providers::{self as providers, FailureKind, Media, MediaSource, Publication, Step};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;
pub enum BridgeOutcome {
    Published {
        external_id: String,
        external_url: Option<String>,
        receipt: Value,
    },
    Uncertain {
        reason: String,
    },
    Rejected {
        reason: String,
    },
}
fn encryption_key(state: &AppState) -> Result<&[u8; 32]> {
    state
        .config
        .encryption_key
        .as_ref()
        .ok_or_else(|| ApiError::bad("Server encryption key is required for provider access"))
}

mod delivery;
mod execution;
mod preparation;
pub use execution::publish;
mod inbox;
mod media;
mod observations;
pub use inbox::*;
pub use media::*;
pub use observations::*;

mod reconciliation;
pub use reconciliation::*;

mod reply_recovery;
pub use reply_recovery::*;

mod reply_claim;
