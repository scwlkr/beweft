use super::*;
pub async fn start(
    State(state): State<AppState>,
    auth: Auth,
    headers: HeaderMap,
    Path(provider): Path<String>,
    Json(body): Json<Value>,
) -> Result<Response> {
    auth.owner_session()?;
    let brand = uuid(&body, "brand_id")?;
    auth.brand(brand)?;
    let reconnect = body["reconnect_channel_id"]
        .as_str()
        .map(|v| {
            v.parse::<Uuid>()
                .map_err(|_| ApiError::bad("Invalid reconnect channel"))
        })
        .transpose()?;
    if let Some(id) = reconnect {
        let row = sqlx::query("SELECT brand_id,provider FROM channels WHERE id=$1")
            .bind(id)
            .fetch_one(&state.db)
            .await?;
        if row.get::<Uuid, _>("brand_id") != brand || row.get::<String, _>("provider") != provider {
            return Err(ApiError::bad(
                "Reconnect target must match the brand and provider",
            ));
        }
    }
    let id = Uuid::new_v4();
    let state_token = crypto::random_secret();
    let correlation = crypto::random_secret();
    let verifier = crypto::random_secret();
    let mut tx = state.db.begin().await?;
    let generation = super::apps::require_open_oauth_gate(
        super::apps::oauth_gate(&mut tx, &provider, false).await?,
    )?;
    let app = super::apps::app_config_tx(&state, &mut tx, &provider).await?;
    let redirect = format!(
        "{}/api/v1/connections/{provider}/callback",
        state.config.public_url
    );
    let authorization_url = providers::authorization_url(&app, &redirect, &state_token, &verifier)
        .map_err(provider_error)?;
    let encrypted = crypto::encrypt(key(&state)?, id, &verifier).map_err(ApiError::internal)?;
    let encrypted_app = crypto::encrypt(
        key(&state)?,
        id,
        &serde_json::to_string(&app).map_err(ApiError::internal)?,
    )
    .map_err(ApiError::internal)?;
    sqlx::query("DELETE FROM connection_flows WHERE expires_at<now()")
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO connection_flows(id,provider,owner_id,brand_id,session_hash,state_hash,correlation_hash,encrypted_verifier,encrypted_app,reconnect_channel_id,oauth_generation,expires_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,now()+interval '10 minutes')").bind(id).bind(&provider).bind(auth.owner_id).bind(brand).bind(session_hash(&headers)?).bind(crypto::digest(&state_token)).bind(crypto::digest(&correlation)).bind(encrypted).bind(encrypted_app).bind(reconnect).bind(generation).execute(&mut *tx).await?;
    tx.commit().await?;
    let mut response =
        data(json!({"authorization_url":authorization_url,"flow_id":id})).into_response();
    response.headers_mut().insert(header::SET_COOKIE,format!("beweft_oauth={correlation}; HttpOnly; SameSite=Lax; Path=/api/v1/connections; Max-Age=600{}",if state.config.secure_cookie{"; Secure"}else{""}).parse().map_err(ApiError::internal)?);
    Ok(response)
}
#[derive(Deserialize)]
pub struct Callback {
    pub state: Option<String>,
    pub code: Option<String>,
    pub error: Option<String>,
}
pub async fn callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(provider): Path<String>,
    Query(query): Query<Callback>,
) -> Result<Response> {
    let state_value = query
        .state
        .filter(|s| s.len() <= 256)
        .ok_or_else(|| ApiError::bad("Missing OAuth state; start again"))?;
    let correlation = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| {
            s.split(';')
                .find_map(|p| p.trim().strip_prefix("beweft_oauth="))
        })
        .ok_or_else(|| ApiError::bad("Connection browser session expired; start again"))?;
    let row=sqlx::query("UPDATE connection_flows f SET consumed_at=now() WHERE state_hash=$1 AND correlation_hash=$2 AND provider=$3 AND expires_at>now() AND consumed_at IS NULL AND EXISTS(SELECT 1 FROM sessions s WHERE s.token_hash=f.session_hash AND s.owner_id=f.owner_id AND s.expires_at>now()) RETURNING id,encrypted_verifier,encrypted_app,oauth_generation").bind(crypto::digest(&state_value)).bind(crypto::digest(correlation)).bind(&provider).fetch_optional(&state.db).await?.ok_or_else(||ApiError::bad("Connection expired or was already used; start again"))?;
    if query.error.is_some() {
        return Ok(Redirect::to("/settings?connection_error=consent_declined").into_response());
    }
    let id: Uuid = row.get("id");
    let code = query
        .code
        .filter(|s| s.len() <= 8192)
        .ok_or_else(|| ApiError::bad("Provider did not return an authorization code"))?;
    let verifier = crypto::decrypt(key(&state)?, id, row.get("encrypted_verifier"))
        .map_err(|_| ApiError::bad("Connection state could not be decrypted"))?;
    let app: AppConfig = serde_json::from_str(
        &crypto::decrypt(
            key(&state)?,
            id,
            row.get::<Option<String>, _>("encrypted_app")
                .as_deref()
                .ok_or_else(|| {
                    ApiError::bad("Connection started before upgrade; authorize again")
                })?,
        )
        .map_err(ApiError::internal)?,
    )
    .map_err(ApiError::internal)?;
    // Serialize the entire external exchange with disconnect and manual removal.
    // Old or pre-upgrade flows must fail before contacting the provider.
    let mut tx = state.db.begin().await?;
    let generation = super::apps::oauth_gate(&mut tx, &provider, false)
        .await
        .and_then(super::apps::require_open_oauth_gate)
        .map_err(|error| {
            if error.0 == axum::http::StatusCode::CONFLICT {
                ApiError::conflict("This authorization was consumed. If provider access removal is pending, complete and confirm it; otherwise wait for the current operation to finish, then start a new authorization.")
            } else {
                error
            }
        })?;
    if row.get::<Option<i64>, _>("oauth_generation") != Some(generation) {
        return Err(ApiError::conflict(
            "Provider access changed during authorization; start again",
        ));
    }
    if !app
        .same_grant_authority(&super::apps::locked_app_config_tx(&state, &mut tx, &provider).await?)
    {
        return Err(ApiError::bad(
            "Provider configuration changed during authorization; start again",
        ));
    }

    let candidates = tokio::time::timeout(std::time::Duration::from_secs(60), async {
        let adapter = adapter_for(&app).await?;
        let tokens = adapter
            .exchange(
                &app,
                &code,
                &format!("{}/api/v1/connections/{provider}/callback", state.config.public_url),
                &verifier,
            )
            .await
            .map_err(|error| ApiError::bad(format!("{} If access was granted remotely, remove it at the provider before starting again.", error.message)))?;
        adapter.identities(&app, &tokens).await.map_err(|error| ApiError::bad(format!("{} Authorization may have granted access; remove it at the provider before starting again.", error.message)))
    })
    .await
    .map_err(|_| ApiError::conflict("Provider authorization timed out and its outcome is unknown. Check and remove any app access at the provider before starting again."))??;
    if candidates.is_empty() {
        return Err(ApiError::bad(
            "No eligible destination was returned. Check account eligibility and permissions; if access was granted, remove it at the provider before starting again.",
        ));
    }
    let encrypted = crypto::encrypt(
        key(&state)?,
        id,
        &serde_json::to_string(&candidates).map_err(ApiError::internal)?,
    )
    .map_err(ApiError::internal)?;
    sqlx::query("UPDATE connection_flows SET encrypted_candidates=$2 WHERE id=$1")
        .bind(id)
        .bind(encrypted)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    let mut response = Redirect::to(&format!("/settings?connection_flow={id}")).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        "beweft_oauth=; HttpOnly; SameSite=Lax; Path=/api/v1/connections; Max-Age=0"
            .parse()
            .unwrap(),
    );
    Ok(response)
}
async fn flow(
    state: &AppState,
    auth: &Auth,
    headers: &HeaderMap,
    id: Uuid,
) -> Result<(sqlx::postgres::PgRow, Vec<Channel>)> {
    auth.owner_session()?;
    let row=sqlx::query("SELECT * FROM connection_flows WHERE id=$1 AND owner_id=$2 AND session_hash=$3 AND expires_at>now() AND completed_at IS NULL").bind(id).bind(auth.owner_id).bind(session_hash(headers)?).fetch_optional(&state.db).await?.ok_or_else(||ApiError::bad("Connection flow expired; start again"))?;
    let encrypted: Option<String> = row.get("encrypted_candidates");
    let plain = crypto::decrypt(
        key(state)?,
        id,
        &encrypted.ok_or_else(|| ApiError::bad("Provider authorization has not completed"))?,
    )
    .map_err(|_| ApiError::bad("Connection candidates could not be decrypted"))?;
    let candidates = serde_json::from_str(&plain)
        .map_err(|_| ApiError::bad("Stored connection candidates are invalid"))?;
    Ok((row, candidates))
}
pub async fn flow_destinations(
    State(state): State<AppState>,
    auth: Auth,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let (row, candidates) = flow(&state, &auth, &headers, id).await?;
    Ok(data(
        json!({"id":id,"brand_id":row.get::<Uuid,_>("brand_id"),"provider":row.get::<String,_>("provider"),"destinations":candidates.iter().map(|c|&c.identity).collect::<Vec<_>>()}),
    ))
}
pub async fn complete(
    State(state): State<AppState>,
    auth: Auth,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<Value>,
) -> Result<Json<Value>> {
    let (row, candidates) = flow(&state, &auth, &headers, id).await?;
    let identity = required(&body, "destination_id")?;
    let candidate = candidates
        .into_iter()
        .find(|c| c.identity.id == identity)
        .ok_or_else(|| ApiError::bad("Choose one of the authorized destinations"))?;
    let provider: String = row.get("provider");
    if candidate.app.provider != provider {
        return Err(ApiError::bad(
            "Connection provider does not match; authorize again",
        ));
    }
    let brand: Uuid = row.get("brand_id");
    let reconnect: Option<Uuid> = row.get("reconnect_channel_id");
    let timezone = body["timezone"].as_str().unwrap_or("UTC");
    catalog::validate_timezone(timezone)?;
    let mut tx = state.db.begin().await?;
    let generation = super::apps::require_open_oauth_gate(
        super::apps::oauth_gate(&mut tx, &provider, false).await?,
    )?;
    if row.get::<Option<i64>, _>("oauth_generation") != Some(generation) {
        return Err(ApiError::conflict(
            "Provider access changed during authorization; start again",
        ));
    }
    if !candidate
        .app
        .same_grant_authority(&super::apps::locked_app_config_tx(&state, &mut tx, &provider).await?)
    {
        return Err(ApiError::bad(
            "Provider configuration changed; authorize again",
        ));
    }
    let changed=sqlx::query("UPDATE connection_flows SET completed_at=now(),encrypted_candidates=NULL WHERE id=$1 AND completed_at IS NULL AND expires_at>now()").bind(id).execute(&mut *tx).await?.rows_affected();
    if changed != 1 {
        return Err(ApiError::conflict(
            "This connection was completed or expired",
        ));
    }
    let result = store_channel(&state, &mut tx, brand, reconnect, timezone, &candidate).await?;
    tx.commit().await?;
    Ok(data(result))
}
