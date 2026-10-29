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
    /// A completed, non-public checkpoint is ready for a later bounded claim.
    Continue,
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
pub(crate) use delivery::save_checkpoint_tx as save_recovery_checkpoint_tx;
pub use execution::publish;
mod validation;
pub use validation::ScheduleFormatValidator;
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

// Session and transaction advisory locks use the same namespaced key. The
// session owner keeps it across the provider call; recovery must never queue.
pub(crate) fn reply_execution_key(inbox: Uuid) -> String {
    format!("reply-execution:{inbox}")
}

pub(crate) async fn try_reply_execution_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    inbox: Uuid,
) -> Result<()> {
    let acquired: bool =
        sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtextextended($1,0))")
            .bind(reply_execution_key(inbox))
            .fetch_one(&mut **tx)
            .await?;
    if !acquired {
        return Err(ApiError::conflict_code(
            "reply_execution_active",
            "The original reply sender is still active. Wait for it to stop, then reload reply history and inspect the provider before resolving the outcome.",
        ));
    }
    Ok(())
}
