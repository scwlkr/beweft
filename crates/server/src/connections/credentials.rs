use super::*;
pub(super) async fn store_channel(
    state: &AppState,
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    brand: Uuid,
    reconnect: Option<Uuid>,
    timezone: &str,
    channel: &Channel,
) -> Result<Value> {
    let id = reconnect.unwrap_or_else(Uuid::new_v4);
    if let Some(existing) = reconnect {
        let identity:String=sqlx::query_scalar("SELECT connection_identity FROM channels WHERE id=$1 AND brand_id=$2 AND provider=$3 FOR UPDATE").bind(existing).bind(brand).bind(&channel.app.provider).fetch_one(&mut **tx).await?;
        if identity != channel.destination_key() {
            return Err(ApiError::bad(
                "Reconnect must authorize the same destination. Connect another channel separately.",
            ));
        }
    }
    let encrypted = crypto::encrypt(
        key(state)?,
        id,
        &serde_json::to_string(channel).map_err(ApiError::internal)?,
    )
    .map_err(ApiError::internal)?;
    let mut caps = serde_json::to_value(
        providers::capabilities()
            .into_iter()
            .find(|c| c.id == channel.app.provider)
            .ok_or_else(|| ApiError::bad("Unknown provider"))?,
    )
    .map_err(ApiError::internal)?;
    caps["execution"] = json!("direct");
    caps["verification"] = json!("identity_verified_publishing_unverified");
    caps["permissions"] = providers::grant_permissions(channel);
    caps["publishing"] = json!(caps["permissions"]["actions"]["publish"]["status"] != "missing");
    let row:Value=sqlx::query_scalar("INSERT INTO channels(id,brand_id,name,provider,timezone,capabilities,encrypted_credentials,connection_identity,connection_status,connected_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,'connected',now()) ON CONFLICT(id) DO UPDATE SET name=EXCLUDED.name,capabilities=EXCLUDED.capabilities,encrypted_credentials=EXCLUDED.encrypted_credentials,connection_status='connected',connection_error=NULL,connected_at=now() RETURNING to_jsonb(channels.*)-'encrypted_credentials'").bind(id).bind(brand).bind(&channel.identity.name).bind(&channel.app.provider).bind(timezone).bind(caps).bind(encrypted).bind(channel.destination_key()).fetch_one(&mut **tx).await?;
    Ok(crate::manual_channels::decorate(row))
}

async fn check_bluesky_reconnect(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: Uuid,
    brand: Uuid,
    lock: bool,
) -> Result<()> {
    let query = if lock {
        "SELECT brand_id,provider FROM channels WHERE id=$1 FOR UPDATE"
    } else {
        "SELECT brand_id,provider FROM channels WHERE id=$1"
    };
    let target = sqlx::query(query)
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| ApiError::bad("Reconnect target is missing"))?;
    if target.get::<Uuid, _>("brand_id") != brand
        || target.get::<String, _>("provider") != "bluesky"
    {
        return Err(ApiError::bad(
            "Reconnect target must match the brand and provider",
        ));
    }
    Ok(())
}

pub async fn bluesky(
    State(state): State<AppState>,
    auth: Auth,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>> {
    auth.owner_session()?;
    let brand = uuid(&body, "brand_id")?;
    auth.brand(brand)?;
    let identifier = required(&body, "identifier")?;
    let password = required(&body, "app_password")?;
    if identifier.len() > 512 || password.len() > 512 {
        return Err(ApiError::bad("Credentials are too long"));
    }
    let reconnect = body["reconnect_channel_id"]
        .as_str()
        .map(|s| {
            s.parse()
                .map_err(|_| ApiError::bad("Invalid reconnect channel"))
        })
        .transpose()?;
    let timezone = body["timezone"].as_str().unwrap_or("UTC");
    catalog::validate_timezone(timezone)?;
    let browser_session = session_hash(&headers)?;
    let mut tx = state.db.begin().await?;
    // Keep the open generation and app authority locked across session creation
    // and storage, so a disconnect/configuration change cannot cross them.
    super::apps::require_open_oauth_gate(super::apps::oauth_gate(&mut tx, "bluesky").await?)?;
    // Check before the remote write, then serialize and recheck after it.
    // An archive can therefore finish while the provider is slow; it wins
    // and prevents the new session from being stored.
    check_active_brand(&mut tx, brand).await?;
    let app = super::apps::locked_app_config_tx(&state, &mut tx, "bluesky").await?;
    if let Some(id) = reconnect {
        check_bluesky_reconnect(&mut tx, id, brand, false).await?;
    }
    let started = std::time::Instant::now();
    let remaining: Option<f64> = sqlx::query_scalar("SELECT EXTRACT(EPOCH FROM (expires_at-clock_timestamp()))::double precision FROM sessions WHERE token_hash=$1 AND owner_id=$2")
        .bind(&browser_session).bind(auth.owner_id).fetch_optional(&mut *tx).await?;
    let deadline = super::flows::authorization_deadline(started, remaining)?;
    let channel = tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), async {
        adapter_for(&app)
            .await?
            .with_monotonic_deadline(deadline)
            .connect_bluesky(&app, &identifier, &password)
            .await
            .map_err(provider_error)
    })
    .await
    .map_err(|_| {
        ApiError::conflict(super::flows::recovery(
            "Bluesky session creation timed out and its outcome is unknown.",
        ))
    })?
    .map_err(super::flows::post_grant_error)?;
    lock_active_brand(&mut tx, brand)
        .await
        .map_err(super::flows::post_grant_error)?;
    if let Some(id) = reconnect {
        crate::schedules::lock_channel(&mut tx, id)
            .await
            .map_err(super::flows::post_grant_error)?;
        check_bluesky_reconnect(&mut tx, id, brand, true)
            .await
            .map_err(super::flows::post_grant_error)?;
    }
    let result = store_channel(&state, &mut tx, brand, reconnect, timezone, &channel)
        .await
        .map_err(super::flows::post_grant_error)?;
    let session_is_live: Option<i32> = sqlx::query_scalar("SELECT 1 FROM sessions WHERE token_hash=$1 AND owner_id=$2 AND expires_at>clock_timestamp() FOR SHARE")
        .bind(browser_session)
        .bind(auth.owner_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| ApiError::conflict(super::flows::recovery("Bluesky session creation finished, but the browser session could not be verified.")))?;
    if session_is_live.is_none() {
        return Err(ApiError::conflict(super::flows::recovery(
            "The browser session ended before this connection could be saved.",
        )));
    }
    tx.commit().await.map_err(|_| {
        ApiError::conflict(super::flows::recovery(
            "Bluesky session creation finished, but saving the connection could not be confirmed.",
        ))
    })?;
    Ok(data(result))
}
/// Serialize refresh under a channel row lock. Rotated tokens commit atomically.
pub async fn load_channel(state: &AppState, id: Uuid) -> Result<Channel> {
    let mut tx = state.db.begin().await?;
    let result = load_channel_tx(state, &mut tx, id, None).await;
    tx.commit().await?;
    result
}
/// Callers with locks must reuse their connection, never acquire a second pool slot.
pub async fn load_channel_tx(
    state: &AppState,
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: Uuid,
    deadline: Option<std::time::Instant>,
) -> Result<Channel> {
    load_channel_inner(state, tx, id, deadline, true).await
}
/// Never refresh inside a downstream transaction whose rollback could discard a rotated grant.
pub async fn load_stored_channel_tx(
    state: &AppState,
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: Uuid,
) -> Result<Channel> {
    load_channel_inner(state, tx, id, None, false).await
}
async fn load_channel_inner(
    state: &AppState,
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: Uuid,
    deadline: Option<std::time::Instant>,
    refresh: bool,
) -> Result<Channel> {
    let row=sqlx::query("SELECT provider,encrypted_credentials,connection_status FROM channels WHERE id=$1 FOR UPDATE").bind(id).fetch_one(&mut **tx).await?;
    if row.get::<String, _>("connection_status") != "connected" {
        return Err(ApiError::bad("Channel needs reconnection"));
    }
    let encrypted: Option<String> = row.get("encrypted_credentials");
    let plain = crypto::decrypt(
        key(state)?,
        id,
        &encrypted.ok_or_else(|| ApiError::bad("Channel has no credentials"))?,
    )
    .map_err(|_| ApiError::bad("Channel credentials could not be decrypted"))?;
    let mut channel: Channel = serde_json::from_str(&plain)
        .map_err(|_| ApiError::bad("Invalid stored channel credentials"))?;
    let current = super::apps::app_config_tx(state, tx, &row.get::<String, _>("provider")).await?;
    if !channel.app.same_grant_authority(&current) {
        sqlx::query("UPDATE channels SET connection_status='reconnect_required',connection_error='Provider origin or client changed; reconnect before using this grant' WHERE id=$1").bind(id).execute(&mut **tx).await?;
        return Err(ApiError::bad(
            "Provider app changed. Reconnect this channel; stored tokens cannot move between origins or clients.",
        ));
    }
    channel.app.client_secret = current.client_secret;
    if refresh
        && channel
            .tokens
            .expires_at
            .is_some_and(|t| t < chrono::Utc::now().timestamp() + 300)
    {
        let mut adapter = if let Some(deadline) = deadline {
            if deadline <= std::time::Instant::now() {
                return Err(ApiError::conflict(
                    "Delivery lease expired before provider setup",
                ));
            }
            tokio::time::timeout_at(
                tokio::time::Instant::from_std(deadline),
                adapter_for(&channel.app),
            )
            .await
            .map_err(|_| ApiError::conflict("Delivery lease expired during provider setup"))??
        } else {
            adapter_for(&channel.app).await?
        };
        if let Some(deadline) = deadline {
            adapter = adapter.with_monotonic_deadline(deadline);
        }
        match adapter.refresh(&channel.app, &channel.tokens).await {
            Ok(tokens) => {
                channel.tokens = tokens;
                let encrypted = crypto::encrypt(
                    key(state)?,
                    id,
                    &serde_json::to_string(&channel).map_err(ApiError::internal)?,
                )
                .map_err(ApiError::internal)?;
                sqlx::query("UPDATE channels SET encrypted_credentials=$2,connection_error=NULL,capabilities=jsonb_set(capabilities,'{permissions}',$3) WHERE id=$1").bind(id).bind(encrypted).bind(providers::grant_permissions(&channel)).execute(&mut **tx).await?;
            }
            Err(e) => {
                if e.kind == providers::FailureKind::Authentication
                    || e.kind == providers::FailureKind::Rejected
                {
                    sqlx::query("UPDATE channels SET connection_status='reconnect_required',connection_error=$2 WHERE id=$1").bind(id).bind(&e.message).execute(&mut **tx).await?;
                }
                return Err(provider_error(e));
            }
        }
    }
    Ok(channel)
}
/// The public route requires explicit acknowledgment before any disconnect mutation.
pub async fn disconnect_confirmed(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(body): Json<crate::contracts::DisconnectRequest>,
) -> Result<Json<Value>> {
    auth.owner_session()?;
    if !body.confirm_shared_impact {
        return Err(ApiError::bad_code(
            "disconnect_confirmation_required",
            "Confirm that this channel's queued deliveries will be canceled and other channels for this provider, including unrelated accounts, may need reconnection after manual access removal.",
        ));
    }
    disconnect(State(state), auth, Path(id)).await
}

pub async fn disconnect(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    auth.owner_session()?;
    auth.brand(catalog::channel_brand(&state, id).await?)?;
    let provider: String = sqlx::query_scalar("SELECT provider FROM channels WHERE id=$1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    // The provider transaction gate orders local credential removal with
    // authorization and configuration. Disconnect never sends a revoke.
    let mut tx = state.db.begin().await?;
    let gate = super::apps::oauth_gate(&mut tx, &provider).await?;
    if gate.is_some_and(|(_, pending)| pending.is_some()) {
        return Err(super::apps::pending_removal_error());
    }
    // Direct grants may be shared. An unhealthy or otherwise unknown direct
    // channel can still have provider access even without stored ciphertext.
    // Read a hint, then acquire the channel locks in stable order.
    let target = sqlx::query(
        "SELECT encrypted_credentials IS NOT NULL AS has_grant,connection_status FROM channels WHERE id=$1",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    let has_grant_hint: bool = target.get("has_grant");
    if !has_grant_hint && target.get::<String, _>("connection_status") == "disconnected" {
        // A prior completed disconnect already crossed the removal boundary.
        // Do not revoke a fresh sibling's grant when this request is repeated.
        tx.commit().await?;
        return Ok(data(json!({
            "disconnected":true,
            "provider_revoked":false,
            "channels_requiring_reconnection":0,
            "revocation_id":null,
            "next_action":"This channel is already disconnected. No new provider access removal was started."
        })));
    }
    let provider_wide = has_grant_hint
        || (providers::known(&provider)
            && target.get::<String, _>("connection_status") != "disconnected");
    let affected: Vec<Uuid> = if provider_wide {
        sqlx::query_scalar("SELECT id FROM channels WHERE provider=$1 ORDER BY id")
            .bind(&provider)
            .fetch_all(&mut *tx)
            .await?
    } else {
        vec![id]
    };
    for channel_id in &affected {
        crate::schedules::lock_channel(&mut tx, *channel_id).await?;
    }
    let target = sqlx::query(
        "SELECT encrypted_credentials IS NOT NULL AS has_grant,connection_status FROM channels WHERE id=$1 FOR UPDATE",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    let grant_may_exist = target.get::<bool, _>("has_grant")
        || (providers::known(&provider)
            && target.get::<String, _>("connection_status") != "disconnected");
    if grant_may_exist && !provider_wide {
        return Err(ApiError::conflict(
            "Channel access changed during disconnect; try again",
        ));
    }
    // Provider HTTP calls have a 60-second timeout. A crashed reply remains uncertain,
    // never retried automatically; it must not block disconnect forever.
    sqlx::query("UPDATE provider_replies r SET state='uncertain',error='Reply process stopped; inspect the network before a new reply',updated_at=now() FROM provider_inbox i WHERE i.id=r.inbox_id AND i.channel_id=ANY($1) AND r.state='sending' AND r.delivery_deadline<clock_timestamp()").bind(&affected).execute(&mut *tx).await?;
    let sending: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM publications WHERE channel_id=ANY($1) AND state='sending') OR EXISTS(SELECT 1 FROM provider_replies r JOIN provider_inbox i ON i.id=r.inbox_id WHERE i.channel_id=ANY($1) AND r.state='sending')",
    )
    .bind(&affected)
    .fetch_one(&mut *tx)
    .await?;
    if sending {
        return Err(ApiError::conflict(
            "A delivery is in progress. Wait for its outcome before disconnecting.",
        ));
    }
    // Readiness, decryptability and missing ciphertext do not establish that
    // provider access is absent. Owner-managed removal is required for any
    // known or possible direct grant, regardless of provider.
    let revocation_id = grant_may_exist.then(Uuid::new_v4);
    let affected_channels = if revocation_id.is_some() {
        sqlx::query("UPDATE channels SET connection_status='reconnect_required',connection_error='Provider access may change; reconnect this channel after removal is resolved' WHERE provider=$2 AND id<>$1 AND connection_status='connected'")
            .bind(id).bind(&provider).execute(&mut *tx).await?.rows_affected()
    } else {
        0
    };
    if gate.is_some() {
        sqlx::query("UPDATE provider_oauth_fences SET oauth_generation=oauth_generation+1,revocation_id=$2 WHERE provider=$1")
            .bind(&provider)
            .bind(revocation_id)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("UPDATE channels SET encrypted_credentials=NULL,connection_status='disconnected',paused=true,connection_error=NULL WHERE id=$1").bind(id).execute(&mut *tx).await?;
    sqlx::query("UPDATE publications SET state='canceled',last_error='Channel disconnected by owner',updated_at=now() WHERE channel_id=$1 AND state='queued'").bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    let next_action = match (affected_channels > 0, revocation_id.is_some()) {
        (true, true) => {
            "Other channels for this provider, including unrelated accounts, were conservatively marked for reconnection. Removing provider access may affect them. First inspect your channels, remove beweft access in the provider account's access settings (such as connected apps or app passwords), then confirm removal before connecting again; their saved credentials and queued work were retained."
        }
        (false, true) => {
            "Provider access removal can affect other channels or accounts. First inspect connected channels, remove beweft access in the provider account's access settings (such as connected apps or app passwords), then confirm removal before connecting again."
        }
        (_, false) => {
            "No remote removal was attempted. Inspect existing connected channels before removing only orphaned provider access."
        }
    };
    Ok(data(
        json!({"disconnected":true,"provider_revoked":false,"channels_requiring_reconnection":affected_channels,"revocation_id":revocation_id,"next_action":next_action}),
    ))
}

/// Record the owner’s attestation that provider access was removed manually.
pub async fn acknowledge_revocation(
    State(state): State<AppState>,
    auth: Auth,
    Path(provider): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>> {
    auth.owner_session()?;
    if body["access_removed"] != true {
        return Err(ApiError::bad(
            "Confirm that provider access was removed before connecting again",
        ));
    }
    let mut tx = state.db.begin().await?;
    let gate = super::apps::oauth_gate(&mut tx, &provider).await?;
    let expected = uuid(&body, "revocation_id")?;
    if !gate.is_some_and(|(_, pending)| pending == Some(expected)) {
        return Err(ApiError::conflict_code(
            "revocation_id_conflict",
            "The provider removal changed or is no longer pending. Reload provider status before confirming removal.",
        ));
    }
    sqlx::query("UPDATE provider_oauth_fences SET oauth_generation=oauth_generation+1,revocation_id=NULL WHERE provider=$1 AND revocation_id=$2")
        .bind(&provider)
        .bind(expected)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(data(json!({"revocation_acknowledged":true})))
}
