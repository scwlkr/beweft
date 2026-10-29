use super::*;
/// Serialize OAuth completion and revocation for the provider, including remote I/O.
/// A busy gate fails promptly so other requests do not wait on the provider timeout.
pub(super) async fn oauth_gate(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    provider: &str,
    wait: bool,
) -> Result<Option<(i64, Option<Uuid>)>> {
    fn gate_error(error: sqlx::Error) -> ApiError {
        if matches!(&error, sqlx::Error::Database(db) if db.code().as_deref() == Some("55P03")) {
            ApiError::conflict(
                "Provider revocation or another connection operation is in progress; try again",
            )
        } else {
            error.into()
        }
    }
    sqlx::query("SET LOCAL lock_timeout = '5s'")
        .execute(&mut **tx)
        .await
        .map_err(gate_error)?;
    let query = if wait {
        "SELECT oauth_generation,revocation_id FROM provider_oauth_fences WHERE provider=$1 FOR UPDATE"
    } else {
        "SELECT oauth_generation,revocation_id FROM provider_oauth_fences WHERE provider=$1 FOR UPDATE NOWAIT"
    };
    let mut row = sqlx::query(query)
        .bind(provider)
        .fetch_optional(&mut **tx)
        .await
        .map_err(gate_error)?;
    if row.is_none() {
        sqlx::query("INSERT INTO provider_oauth_fences(provider) VALUES($1) ON CONFLICT(provider) DO NOTHING")
            .bind(provider).execute(&mut **tx).await.map_err(gate_error)?;
        row = sqlx::query(query)
            .bind(provider)
            .fetch_optional(&mut **tx)
            .await
            .map_err(gate_error)?;
    }
    Ok(row.map(|row| (row.get("oauth_generation"), row.get("revocation_id"))))
}

pub(super) fn require_open_oauth_gate(gate: Option<(i64, Option<Uuid>)>) -> Result<i64> {
    match gate {
        Some((generation, None)) => Ok(generation),
        Some((_, Some(_))) => Err(ApiError::conflict(
            "Provider revocation is not confirmed. Remove access at the provider and confirm it before connecting again.",
        )),
        None => Err(ApiError::bad("Set up this provider app first")),
    }
}

pub async fn app_config(state: &AppState, provider: &str) -> Result<AppConfig> {
    let row = sqlx::query("SELECT id,encrypted_config FROM provider_apps WHERE provider=$1")
        .bind(provider)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| ApiError::bad("Set up this provider app first"))?;
    decode_app(state, row)
}
pub(super) async fn app_config_tx(
    state: &AppState,
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    provider: &str,
) -> Result<AppConfig> {
    let row = sqlx::query("SELECT id,encrypted_config FROM provider_apps WHERE provider=$1")
        .bind(provider)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| ApiError::bad("Set up this provider app first"))?;
    decode_app(state, row)
}
pub(super) async fn locked_app_config_tx(
    state: &AppState,
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    provider: &str,
) -> Result<AppConfig> {
    let row =
        sqlx::query("SELECT id,encrypted_config FROM provider_apps WHERE provider=$1 FOR SHARE")
            .bind(provider)
            .fetch_optional(&mut **tx)
            .await?
            .ok_or_else(|| ApiError::bad("Set up this provider app first"))?;
    decode_app(state, row)
}
fn decode_app(state: &AppState, row: sqlx::postgres::PgRow) -> Result<AppConfig> {
    let plain =
        crypto::decrypt(key(state)?, row.get("id"), row.get("encrypted_config")).map_err(|_| {
            ApiError::bad(
                "Provider credentials could not be decrypted; check the server encryption key",
            )
        })?;
    serde_json::from_str(&plain)
        .map_err(|_| ApiError::bad("Stored provider configuration is invalid"))
}
fn is_public(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v) => {
            let o = v.octets();
            !v.is_private()
                && !v.is_loopback()
                && !v.is_link_local()
                && !v.is_unspecified()
                && !v.is_multicast()
                && o[0] != 0
                && o[0] < 240
                && !(o[0] == 100 && (64..=127).contains(&o[1]))
                && !(o[0] == 192 && o[1] == 0)
                && !(o[0] == 198 && (o[1] == 18 || o[1] == 19))
        }
        std::net::IpAddr::V6(v) => {
            !v.is_loopback()
                && !v.is_unspecified()
                && !v.is_multicast()
                && !v.is_unique_local()
                && !v.is_unicast_link_local()
                && v.to_ipv4_mapped().is_none()
        }
    }
}
/// Resolve and pin federated hosts to public IPs for this operation, avoiding DNS rebinding.
pub async fn adapter_for(app: &AppConfig) -> Result<Adapter> {
    #[cfg(feature = "provider-fixtures")]
    if let Ok(origin) = std::env::var("BEWEFT_PROVIDER_FIXTURE_ORIGIN") {
        return Adapter::fixture(&origin).map_err(provider_error);
    }
    if matches!(app.provider.as_str(), "mastodon" | "bluesky") {
        let origin = providers::instance_origin(app).map_err(provider_error)?;
        let url =
            url::Url::parse(&origin).map_err(|_| ApiError::bad("Invalid provider instance"))?;
        let host = url.host_str().unwrap();
        let allowed = std::env::var("BEWEFT_ALLOWED_PROVIDER_ORIGINS")
            .unwrap_or_else(|_| "https://bsky.social".into());
        if !allowed
            .split(',')
            .map(str::trim)
            .any(|s| s.trim_end_matches('/') == origin)
        {
            return Err(ApiError::bad(
                "This instance is not in BEWEFT_ALLOWED_PROVIDER_ORIGINS; ask the server operator to allow it",
            ));
        }
        let addresses = tokio::net::lookup_host((host, 443))
            .await
            .map_err(|_| ApiError::bad("Provider instance DNS lookup failed"))?
            .collect::<Vec<_>>();
        if addresses.is_empty() || addresses.iter().any(|a| !is_public(a.ip())) {
            return Err(ApiError::bad(
                "Provider instance must resolve exclusively to public addresses",
            ));
        }
        return Adapter::with_resolved_host(host, &addresses).map_err(provider_error);
    }
    Adapter::new().map_err(provider_error)
}
pub async fn list_apps(State(state): State<AppState>, auth: Auth) -> Result<Json<Value>> {
    auth.owner_session()?;
    let rows:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('provider',provider,'client_id',client_id,'instance_url',instance_url,'configured',true,'updated_at',updated_at) FROM provider_apps ORDER BY provider").fetch_all(&state.db).await?;
    Ok(data(rows))
}
pub async fn save_app(
    State(state): State<AppState>,
    auth: Auth,
    Path(provider): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>> {
    auth.owner_session()?;
    key(&state)?;
    if !providers::known(&provider) {
        return Err(ApiError::bad("Provider is unavailable or manual-only"));
    }
    let mut tx = state.db.begin().await?;
    // Do not queue configuration writes behind an exchange holding the app
    // row; the same gate rejects concurrent edits promptly and preserves the
    // stored revocation fence even when configuration is changed.
    oauth_gate(&mut tx, &provider, false).await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind(format!("provider-app:{provider}"))
        .execute(&mut *tx)
        .await?;
    let old = app_config_tx(&state, &mut tx, &provider).await.ok();
    let client_id = body["client_id"].as_str().unwrap_or("").trim().to_owned();
    if provider != "bluesky" && (client_id.is_empty() || client_id.len() > 512) {
        return Err(ApiError::bad("Client ID is required (maximum 512 bytes)"));
    }
    let secret = body["client_secret"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| old.as_ref().map(|a| a.client_secret.clone()))
        .unwrap_or_default();
    if provider != "bluesky" && (secret.is_empty() || secret.len() > 4096) {
        return Err(ApiError::bad(
            "Client secret is required (maximum 4096 bytes)",
        ));
    }
    let instance = body["instance_url"]
        .as_str()
        .map(|s| s.trim_end_matches('/').to_owned())
        .or_else(|| {
            if provider == "bluesky" {
                Some("https://bsky.social".into())
            } else {
                None
            }
        });
    let app = AppConfig {
        provider: provider.clone(),
        client_id: client_id.clone(),
        client_secret: secret,
        instance_url: instance,
    };
    if matches!(provider.as_str(), "mastodon" | "bluesky") {
        tokio::time::timeout(std::time::Duration::from_secs(10), adapter_for(&app))
            .await
            .map_err(|_| ApiError::bad("Provider instance lookup timed out"))??;
    } else if app.instance_url.is_some() {
        return Err(ApiError::bad(
            "Only Mastodon and Bluesky accept an instance URL",
        ));
    }
    let id: Uuid = sqlx::query_scalar("SELECT id FROM provider_apps WHERE provider=$1")
        .bind(&provider)
        .fetch_optional(&mut *tx)
        .await?
        .unwrap_or_else(Uuid::new_v4);
    let encrypted = crypto::encrypt(
        key(&state)?,
        id,
        &serde_json::to_string(&app).map_err(ApiError::internal)?,
    )
    .map_err(ApiError::internal)?;
    sqlx::query("INSERT INTO provider_apps(id,provider,client_id,instance_url,encrypted_config) VALUES($1,$2,$3,$4,$5) ON CONFLICT(provider) DO UPDATE SET client_id=EXCLUDED.client_id,instance_url=EXCLUDED.instance_url,encrypted_config=EXCLUDED.encrypted_config,updated_at=now()").bind(id).bind(&provider).bind(&client_id).bind(&app.instance_url).bind(encrypted).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(data(
        json!({"provider":provider,"client_id":client_id,"instance_url":app.instance_url,"configured":true,"callback_url":format!("{}/api/v1/connections/{provider}/callback",state.config.public_url)}),
    ))
}
pub async fn descriptors(State(state): State<AppState>, _auth: Auth) -> Result<Json<Value>> {
    let configured: Vec<String> = sqlx::query_scalar("SELECT provider FROM provider_apps")
        .fetch_all(&state.db)
        .await?;
    let mut rows = vec![
        json!({"id":"manual","name":"Manual handoff","implementation":"available","configuration":"ready","execution":"manual","verification":"owner_attestation","formats":["text","image","video"],"reason":"Prepare content and confirm publication yourself."}),
    ];
    for c in providers::capabilities() {
        let ready = configured.contains(&c.id.to_owned()) && state.config.encryption_key.is_some();
        let mut v = serde_json::to_value(c).map_err(ApiError::internal)?;
        v["implementation"] = json!("available");
        v["configuration"] = json!(if ready {
            "configured"
        } else {
            "not_configured"
        });
        v["execution"] = json!("direct");
        v["verification"] = json!("unverified_live");
        rows.push(v);
    }
    for (id, name, reason) in [
        (
            "tiktok",
            "TikTok",
            "Private/internal Direct Post tools do not satisfy TikTok intended-use rules. Use manual handoff.",
        ),
        (
            "substack",
            "Substack",
            "A supported public publishing API was not established. Use manual handoff.",
        ),
        (
            "google_business",
            "Google Business Profile",
            "Provider app approval required; direct adapter is not implemented. Use manual handoff.",
        ),
    ] {
        rows.push(json!({"id":id,"name":name,"implementation":"manual_only","configuration":"unavailable","execution":"manual","verification":"owner_attestation","formats":[],"reason":reason}));
    }
    if state.config.fixture_enabled {
        rows.push(json!({"id":"fixture","name":"Isolated fixture","implementation":"available","configuration":"ready","execution":"fixture","verification":"fixture_only","formats":["text","image","video"]}));
    }
    Ok(data(rows))
}
