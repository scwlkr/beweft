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
    Ok(row)
}
pub async fn bluesky(
    State(state): State<AppState>,
    auth: Auth,
    Json(body): Json<Value>,
) -> Result<Json<Value>> {
    auth.owner_session()?;
    let brand = uuid(&body, "brand_id")?;
    auth.brand(brand)?;
    let app = app_config(&state, "bluesky").await?;
    let identifier = required(&body, "identifier")?;
    let password = required(&body, "app_password")?;
    if identifier.len() > 512 || password.len() > 512 {
        return Err(ApiError::bad("Credentials are too long"));
    }
    let channel = adapter_for(&app)
        .await?
        .connect_bluesky(&app, &identifier, &password)
        .await
        .map_err(provider_error)?;
    let reconnect = body["reconnect_channel_id"]
        .as_str()
        .map(|s| {
            s.parse()
                .map_err(|_| ApiError::bad("Invalid reconnect channel"))
        })
        .transpose()?;
    let timezone = body["timezone"].as_str().unwrap_or("UTC");
    catalog::validate_timezone(timezone)?;
    let mut tx = state.db.begin().await?;
    let result = store_channel(&state, &mut tx, brand, reconnect, timezone, &channel).await?;
    tx.commit().await?;
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
    deadline: Option<chrono::DateTime<chrono::Utc>>,
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
    deadline: Option<chrono::DateTime<chrono::Utc>>,
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
        let mut adapter = adapter_for(&channel.app).await?;
        if let Some(deadline) = deadline {
            adapter = adapter.with_deadline(deadline);
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
    let mut tx = state.db.begin().await?;
    sqlx::query("SET LOCAL lock_timeout = '5s'")
        .execute(&mut *tx)
        .await?;
    let gate = super::apps::oauth_gate(&mut tx, &provider, false).await?;
    if gate.is_some_and(|(_, pending)| pending.is_some()) {
        return Err(ApiError::conflict(
            "Provider revocation is already in progress or needs confirmation",
        ));
    }
    // Google revocation affects the account/project grant, not just one YouTube
    // destination. Lock in the same order used by multi-channel scheduling.
    let affected: Vec<Uuid> = if provider == "youtube" {
        sqlx::query_scalar("SELECT id FROM channels WHERE provider='youtube' ORDER BY id")
            .fetch_all(&mut *tx)
            .await?
    } else {
        vec![id]
    };
    for channel_id in &affected {
        crate::schedules::lock_channel(&mut tx, *channel_id).await?;
    }
    sqlx::query("SELECT id FROM channels WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
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
    let channel = load_stored_channel_tx(&state, &mut tx, id).await.ok();
    let revocation_id = channel.as_ref().and(gate.map(|_| Uuid::new_v4()));
    let affected_channels = if provider == "youtube" && revocation_id.is_some() {
        sqlx::query("UPDATE channels SET connection_status='reconnect_required',connection_error='Google grant may have been revoked; reconnect this channel' WHERE provider='youtube' AND id<>$1 AND connection_status='connected'")
            .bind(id).execute(&mut *tx).await?.rows_affected()
    } else {
        0
    };
    if gate.is_some() {
        sqlx::query("UPDATE provider_apps SET oauth_generation=oauth_generation+1,revocation_id=$2 WHERE provider=$1")
            .bind(&provider)
            .bind(revocation_id)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("UPDATE channels SET encrypted_credentials=NULL,connection_status='disconnected',paused=true,connection_error=NULL WHERE id=$1").bind(id).execute(&mut *tx).await?;
    sqlx::query("UPDATE publications SET state='canceled',last_error='Channel disconnected by owner',updated_at=now() WHERE channel_id=$1 AND state='queued'").bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    let revoked = if let (Some(c), Some(revocation_id)) = (channel, revocation_id) {
        // Keep the provider gate locked until remote I/O returns. The committed marker
        // survives crashes and uncertain outcomes; an owner must resolve those manually.
        let mut revoke_tx = state.db.begin().await?;
        sqlx::query("SET LOCAL lock_timeout = '5s'")
            .execute(&mut *revoke_tx)
            .await?;
        let gate = super::apps::oauth_gate(&mut revoke_tx, &provider, true).await?;
        let revoked = if gate.is_some_and(|(_, pending)| pending == Some(revocation_id)) {
            match adapter_for(&c.app).await {
                Ok(adapter) => adapter.revoke(&c).await.unwrap_or(false),
                Err(_) => false,
            }
        } else {
            false
        };
        if revoked {
            sqlx::query("UPDATE provider_apps SET revocation_id=NULL WHERE provider=$1 AND revocation_id=$2")
                .bind(&provider)
                .bind(revocation_id)
                .execute(&mut *revoke_tx)
                .await?;
        }
        revoke_tx.commit().await?;
        revoked
    } else {
        false
    };
    let next_action = match (revoked, affected_channels > 0, revocation_id.is_some()) {
        (true, true, _) => "Reconnect the other YouTube channels marked as needing reconnection.",
        (true, false, _) => "No action needed",
        (false, _, true) => {
            "Remove beweft access in the provider account's connected-app settings, then confirm removal before connecting again."
        }
        (false, _, false) => {
            "No remote revocation was attempted. Remove beweft access in the provider account's connected-app settings."
        }
    };
    Ok(data(
        json!({"disconnected":true,"provider_revoked":revoked,"channels_requiring_reconnection":affected_channels,"next_action":next_action}),
    ))
}

/// Explicit recovery after an unsupported, failed, or interrupted remote revoke.
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
    let gate = super::apps::oauth_gate(&mut tx, &provider, false).await?;
    if !gate.is_some_and(|(_, pending)| pending.is_some()) {
        return Err(ApiError::conflict(
            "There is no unresolved provider revocation",
        ));
    }
    sqlx::query("UPDATE provider_apps SET oauth_generation=oauth_generation+1,revocation_id=NULL WHERE provider=$1")
        .bind(&provider)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(data(json!({"revocation_acknowledged":true})))
}
