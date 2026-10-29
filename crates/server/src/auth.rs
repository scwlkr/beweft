use crate::{
    crypto,
    error::{ApiError, Result, data, required},
    state::AppState,
};
use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use axum::{
    Json,
    extract::{FromRequestParts, State},
    http::{HeaderMap, StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
};
use chrono::{Duration, Utc};
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;
#[derive(Clone)]
pub struct Auth {
    pub owner_id: Uuid,
    pub email: String,
    pub scopes: Vec<String>,
    pub brand_ids: Vec<Uuid>,
    pub session: bool,
}
impl Auth {
    pub fn scope(&self, scope: &str) -> Result<()> {
        if self.session || self.scopes.iter().any(|s| s == scope || s == "admin") {
            Ok(())
        } else {
            Err(ApiError::forbidden())
        }
    }
    pub fn brand(&self, id: Uuid) -> Result<()> {
        if self.brand_ids.is_empty() || self.brand_ids.contains(&id) {
            Ok(())
        } else {
            Err(ApiError::forbidden())
        }
    }
    pub fn owner_session(&self) -> Result<()> {
        if self.session {
            Ok(())
        } else {
            Err(ApiError::forbidden())
        }
    }
}
pub(crate) fn cookie_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|p| p.trim().strip_prefix("beweft_session="))
}
pub fn check_origin(headers: &HeaderMap, state: &AppState, require_header: bool) -> Result<()> {
    if let Some(origin) = headers.get(header::ORIGIN)
        && origin.to_str().ok() != Some(state.config.public_url.as_str())
    {
        return Err(ApiError::forbidden());
    }
    if require_header
        && headers
            .get("x-beweft-request")
            .and_then(|v| v.to_str().ok())
            != Some("1")
    {
        return Err(ApiError::bad("X-Beweft-Request: 1 is required"));
    }
    Ok(())
}
impl FromRequestParts<AppState> for Auth {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self> {
        if let Some(value) = parts.headers.get(header::AUTHORIZATION) {
            let token = value
                .to_str()
                .ok()
                .and_then(|v| v.strip_prefix("Bearer "))
                .ok_or_else(ApiError::unauthorized)?;
            let row=sqlx::query("SELECT t.owner_id,o.email,t.scopes,t.brand_ids FROM api_tokens t JOIN owners o ON o.id=t.owner_id WHERE token_hash=$1").bind(crypto::digest(token)).fetch_optional(&state.db).await?.ok_or_else(ApiError::unauthorized)?;
            return Ok(Self {
                owner_id: row.get("owner_id"),
                email: row.get("email"),
                scopes: serde_json::from_value(row.get("scopes")).map_err(ApiError::internal)?,
                brand_ids: serde_json::from_value(row.get("brand_ids"))
                    .map_err(ApiError::internal)?,
                session: false,
            });
        }
        if !matches!(
            parts.method,
            axum::http::Method::GET | axum::http::Method::HEAD | axum::http::Method::OPTIONS
        ) {
            check_origin(&parts.headers, state, true)?;
        }
        let token = cookie_token(&parts.headers).ok_or_else(ApiError::unauthorized)?;
        let row=sqlx::query("SELECT o.id,o.email FROM sessions s JOIN owners o ON o.id=s.owner_id WHERE s.token_hash=$1 AND s.expires_at>now()").bind(crypto::digest(token)).fetch_optional(&state.db).await?.ok_or_else(ApiError::unauthorized)?;
        Ok(Self {
            owner_id: row.get("id"),
            email: row.get("email"),
            scopes: vec![],
            brand_ids: vec![],
            session: true,
        })
    }
}
pub(crate) async fn limit(state: &AppState, key: String, max: i32) -> Result<()> {
    let attempts:i32=sqlx::query_scalar("INSERT INTO auth_limits(key,attempts) VALUES($1,1) ON CONFLICT(key) DO UPDATE SET attempts=CASE WHEN auth_limits.window_started<now()-interval '15 minutes' THEN 1 ELSE auth_limits.attempts+1 END, window_started=CASE WHEN auth_limits.window_started<now()-interval '15 minutes' THEN now() ELSE auth_limits.window_started END RETURNING attempts").bind(key).fetch_one(&state.db).await?;
    if attempts > max {
        return Err(ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "Too many sign-in attempts; try again in 15 minutes".into(),
        ));
    }
    Ok(())
}
pub async fn status(State(state): State<AppState>) -> Result<Json<Value>> {
    let initialized: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM owners)")
        .fetch_one(&state.db)
        .await?;
    Ok(data(json!({"initialized":initialized})))
}
async fn session_response(
    state: &AppState,
    id: Uuid,
    email: String,
    verified_hash: &str,
) -> Result<Response> {
    let mut tx = state.db.begin().await?;
    // Serialize session creation with password rotation and its session revocation.
    // Argon2 verification happens outside the transaction, so recheck its credential.
    let current: String =
        sqlx::query_scalar("SELECT password_hash FROM owners WHERE id=$1 FOR UPDATE")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
    if current != verified_hash {
        return Err(ApiError::unauthorized());
    }
    let token = crypto::random_secret();
    sqlx::query("INSERT INTO sessions(token_hash,owner_id,expires_at) VALUES($1,$2,$3)")
        .bind(crypto::digest(&token))
        .bind(id)
        .bind(Utc::now() + Duration::days(7))
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    let mut response = data(json!({"id":id,"email":email})).into_response();
    let cookie = format!(
        "beweft_session={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=604800{}",
        if state.config.secure_cookie {
            "; Secure"
        } else {
            ""
        }
    );
    response.headers_mut().insert(
        header::SET_COOKIE,
        cookie.parse().map_err(ApiError::internal)?,
    );
    Ok(response)
}
pub async fn bootstrap(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<crate::contracts::BootstrapRequest>,
) -> Result<Response> {
    let body = serde_json::to_value(input).map_err(ApiError::internal)?;
    check_origin(&headers, &state, true)?;
    limit(&state, "bootstrap".into(), 15).await?;
    if state.config.setup_secret.len() < 16 {
        return Err(ApiError::bad(
            "Server setup requires BEWEFT_SETUP_SECRET with at least 16 characters",
        ));
    }
    let secret = required(&body, "setup_secret")?;
    if !crypto::secrets_equal(&secret, &state.config.setup_secret) {
        return Err(ApiError::forbidden());
    }
    let email = required(&body, "email")?.to_lowercase();
    if !email.contains('@') || email.len() > 254 {
        return Err(ApiError::bad("A valid email address is required"));
    }
    let password = body["password"]
        .as_str()
        .ok_or_else(|| ApiError::bad("password is required"))?
        .to_string();
    if password.chars().count() < 12 || password.len() > 1024 {
        return Err(ApiError::bad(
            "Password must contain at least 12 characters and at most 1,024 bytes",
        ));
    }
    let hash = tokio::task::spawn_blocking(move || {
        Argon2::default()
            .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
            .map(|h| h.to_string())
            .map_err(|_| "password hashing failed")
    })
    .await
    .map_err(ApiError::internal)?
    .map_err(ApiError::internal)?;
    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(9711301)")
        .execute(&mut *tx)
        .await?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM owners)")
        .fetch_one(&mut *tx)
        .await?;
    if exists {
        return Err(ApiError::conflict("Owner is already initialized"));
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO owners(id,email,password_hash) VALUES($1,$2,$3)")
        .bind(id)
        .bind(&email)
        .bind(&hash)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    session_response(&state, id, email, &hash).await
}
pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<crate::contracts::LoginRequest>,
) -> Result<Response> {
    let body = serde_json::to_value(input).map_err(ApiError::internal)?;
    check_origin(&headers, &state, true)?;
    let email = required(&body, "email")?.to_lowercase();
    let password = body["password"]
        .as_str()
        .ok_or_else(|| ApiError::bad("password is required"))?
        .to_string();
    if password.len() > 1024 {
        return Err(ApiError::unauthorized());
    }
    limit(&state, "login_global".into(), 100).await?;
    limit(&state, format!("login:{}", crypto::digest(&email)), 10).await?;
    let row = sqlx::query("SELECT id,email,password_hash FROM owners WHERE email=$1")
        .bind(email)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(ApiError::unauthorized)?;
    let hash: String = row.get("password_hash");
    let verified_hash = hash.clone();
    let valid = tokio::task::spawn_blocking(move || {
        PasswordHash::new(&hash).ok().is_some_and(|h| {
            Argon2::default()
                .verify_password(password.as_bytes(), &h)
                .is_ok()
        })
    })
    .await
    .map_err(ApiError::internal)?;
    if !valid {
        return Err(ApiError::unauthorized());
    }
    session_response(&state, row.get("id"), row.get("email"), &verified_hash).await
}
pub async fn me(auth: Auth) -> Json<Value> {
    data(json!({"id":auth.owner_id,"email":auth.email}))
}
pub async fn logout(
    State(state): State<AppState>,
    auth: Auth,
    headers: HeaderMap,
) -> Result<Response> {
    auth.owner_session()?;
    if let Some(token) = cookie_token(&headers) {
        sqlx::query("DELETE FROM sessions WHERE token_hash=$1")
            .bind(crypto::digest(token))
            .execute(&state.db)
            .await?;
    }
    let mut response = data(json!({"ok":true})).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        "beweft_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0"
            .parse()
            .unwrap(),
    );
    Ok(response)
}
