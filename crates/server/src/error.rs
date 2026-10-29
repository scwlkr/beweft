use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
pub type Result<T> = std::result::Result<T, ApiError>;
#[derive(Debug)]
pub struct ApiError(pub StatusCode, pub &'static str, pub String);
impl ApiError {
    pub fn bad(message: impl Into<String>) -> Self {
        Self(StatusCode::BAD_REQUEST, "invalid_request", message.into())
    }
    pub fn conflict(message: impl Into<String>) -> Self {
        Self(StatusCode::CONFLICT, "conflict", message.into())
    }
    pub fn forbidden() -> Self {
        Self(
            StatusCode::FORBIDDEN,
            "forbidden",
            "This action is not permitted".into(),
        )
    }
    pub fn missing() -> Self {
        Self(
            StatusCode::NOT_FOUND,
            "not_found",
            "Record not found".into(),
        )
    }
    pub fn unauthorized() -> Self {
        Self(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "Sign in to continue".into(),
        )
    }
    pub fn internal(error: impl std::fmt::Display) -> Self {
        tracing::error!(error = %error, "request failed");
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "The request could not be completed".into(),
        )
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.0,
            Json(json!({"error":{"code":self.1,"message":self.2}})),
        )
            .into_response()
    }
}
impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        match &e {
            sqlx::Error::RowNotFound => Self::missing(),
            sqlx::Error::Database(d) if d.is_unique_violation() => {
                Self::conflict("This record or posting slot already exists")
            }
            sqlx::Error::Database(d) if d.is_foreign_key_violation() => {
                Self::bad("A referenced record is missing or still in use")
            }
            _ => Self::internal(e),
        }
    }
}
pub fn data(value: impl serde::Serialize) -> Json<Value> {
    Json(json!({"data":value}))
}
pub fn required(value: &Value, key: &str) -> Result<String> {
    let s = value.get(key).and_then(Value::as_str).unwrap_or("").trim();
    if s.is_empty() || s.len() > 100_000 {
        return Err(ApiError::bad(format!(
            "{key} is required and must be within 100,000 bytes"
        )));
    }
    Ok(s.into())
}
pub fn uuid(value: &Value, key: &str) -> Result<uuid::Uuid> {
    required(value, key)?
        .parse()
        .map_err(|_| ApiError::bad(format!("{key} must be a UUID")))
}
