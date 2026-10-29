#![cfg(feature = "provider-fixtures")]
//! Actual PostgreSQL transactions + HTTP provider fixtures. No live services.
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Path, Query, Request, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::any,
};
use beweft::{
    auth::Auth,
    connections, crypto, provider_bridge,
    state::{AppState, Config},
};
use beweft_providers::{Channel, Identity, Tokens};
use serde_json::{Value, json};
use sqlx::Row;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::Notify;
use uuid::Uuid;
include!("provider_db/harness.rs");
include!("provider_db/connections.rs");
include!("provider_db/recovery.rs");
include!("provider_db/fencing.rs");
include!("provider_db/replies.rs");
include!("provider_db/reply_watermark.rs");
include!("provider_db/durable_preparation.rs");
include!("provider_db/reply_deadline.rs");
include!("provider_db/revocation.rs");
#[path = "../../providers/tests/support/mod.rs"]
mod media_fixture;
#[tokio::test]
#[ignore = "requires isolated BEWEFT_TEST_DATABASE_URL; run provider_integration.py"]
async fn provider_database_security_and_recovery() {
    let h = Harness::new().await;
    oauth_revocation_fences_reconnects(&h).await;
    populated_oauth_fence_upgrade(&h).await;
    oauth_replay_session_binding_and_config_change(&h).await;
    refresh_serialization_and_grant_binding(&h).await;
    replies_and_disconnect_share_claim_lock(&h).await;
    reconciliation_serialization_and_expired_reset(&h).await;
    fenced_writes_and_small_pool(&h).await;
    reply_recovery_and_unresolved_guard(&h).await;
    concurrent_reply_claims_with_small_pool(&h).await;
    delayed_reply_cannot_bypass_terminal_history(&h).await;
    reply_claim_clock_is_independent_of_waits(&h).await;
    refresh_survives_downstream_rollback(&h).await;
    provider_fetches_committed_media_during_post(&h).await;
    h.state.db.close().await;
}
