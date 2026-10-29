use crate::{
    auth::{Auth, cookie_token},
    contracts::PasswordRequest,
    crypto,
    error::{ApiError, Result, data},
    state::AppState,
};
use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use serde_json::{Value, json};
use uuid::Uuid;
fn current(headers: &HeaderMap) -> Result<String> {
    cookie_token(headers)
        .map(crypto::digest)
        .ok_or_else(ApiError::unauthorized)
}
pub async fn sessions(
    State(state): State<AppState>,
    auth: Auth,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    auth.owner_session()?;
    let current = current(&headers)?;
    let rows:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'created_at',created_at,'expires_at',expires_at,'current',token_hash=$2) FROM sessions WHERE owner_id=$1 AND expires_at>now() ORDER BY created_at DESC").bind(auth.owner_id).bind(current).fetch_all(&state.db).await?;
    Ok(data(rows))
}
pub async fn revoke(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    auth.owner_session()?;
    let result = sqlx::query("DELETE FROM sessions WHERE id=$1 AND owner_id=$2")
        .bind(id)
        .bind(auth.owner_id)
        .execute(&state.db)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::missing());
    }
    Ok(data(json!({"ok":true})))
}
pub async fn revoke_others(
    State(state): State<AppState>,
    auth: Auth,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    auth.owner_session()?;
    sqlx::query("DELETE FROM sessions WHERE owner_id=$1 AND token_hash<>$2")
        .bind(auth.owner_id)
        .bind(current(&headers)?)
        .execute(&state.db)
        .await?;
    Ok(data(json!({"ok":true})))
}
pub async fn password(
    State(state): State<AppState>,
    auth: Auth,
    headers: HeaderMap,
    Json(body): Json<PasswordRequest>,
) -> Result<Json<Value>> {
    auth.owner_session()?;
    crate::auth::limit(&state, format!("password:{}", auth.owner_id), 10).await?;
    if body.new_password.chars().count() < 12
        || body.new_password.len() > 1024
        || body.current_password.len() > 1024
    {
        return Err(ApiError::bad(
            "New password must contain at least 12 characters and at most 1,024 bytes",
        ));
    }
    let old: String = sqlx::query_scalar("SELECT password_hash FROM owners WHERE id=$1")
        .bind(auth.owner_id)
        .fetch_one(&state.db)
        .await?;
    let verify_hash = old.clone();
    let new = tokio::task::spawn_blocking(move || {
        let valid = PasswordHash::new(&verify_hash).ok().is_some_and(|h| {
            Argon2::default()
                .verify_password(body.current_password.as_bytes(), &h)
                .is_ok()
        });
        if !valid {
            return Err(ApiError::unauthorized());
        }
        Argon2::default()
            .hash_password(
                body.new_password.as_bytes(),
                &SaltString::generate(&mut OsRng),
            )
            .map(|h| h.to_string())
            .map_err(|_| ApiError::internal("password hashing failed"))
    })
    .await
    .map_err(ApiError::internal)??;
    let mut tx = state.db.begin().await?;
    // Match the session issuer's owner lock before checking or revoking sessions.
    sqlx::query("SELECT id FROM owners WHERE id=$1 FOR UPDATE")
        .bind(auth.owner_id)
        .fetch_one(&mut *tx)
        .await?;
    let current_session:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sessions WHERE owner_id=$1 AND token_hash=$2 AND expires_at>now())").bind(auth.owner_id).bind(current(&headers)?).fetch_one(&mut *tx).await?;
    if !current_session {
        return Err(ApiError::unauthorized());
    }
    let changed =
        sqlx::query("UPDATE owners SET password_hash=$3 WHERE id=$1 AND password_hash=$2")
            .bind(auth.owner_id)
            .bind(old)
            .bind(new)
            .execute(&mut *tx)
            .await?;
    if changed.rows_affected() != 1 {
        return Err(ApiError::conflict(
            "Password changed concurrently; sign in again",
        ));
    }
    sqlx::query("DELETE FROM sessions WHERE owner_id=$1")
        .bind(auth.owner_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO audit_events(id,owner_id,action) VALUES($1,$2,'password_changed')")
        .bind(Uuid::new_v4())
        .bind(auth.owner_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(data(json!({"ok":true})))
}
