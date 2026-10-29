use crate::{
    auth::Auth,
    catalog::Filter,
    error::{ApiError, Result, data},
    state::AppState,
};
use axum::{
    Json,
    body::Body,
    extract::{Multipart, Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;
const MAX_SIZE: usize = 25 * 1024 * 1024;
fn detect(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else if bytes.get(4..8) == Some(b"ftyp") {
        Some("video/mp4")
    } else if bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        Some("video/webm")
    } else {
        None
    }
}
pub async fn list(
    State(state): State<AppState>,
    auth: Auth,
    Query(filter): Query<Filter>,
) -> Result<Json<Value>> {
    if let Some(id) = filter.brand_id {
        auth.brand(id)?;
    }
    let rows:Vec<Value>=sqlx::query_scalar("SELECT (to_jsonb(m)-'storage_key')||jsonb_build_object('url','/api/v1/media/'||id||'/file') FROM media m WHERE ($1::uuid IS NULL OR brand_id=$1) AND (cardinality($2::uuid[])=0 OR brand_id=ANY($2)) ORDER BY created_at DESC").bind(filter.brand_id).bind(&auth.brand_ids).fetch_all(&state.db).await?;
    Ok(data(rows))
}
pub async fn upload(
    State(state): State<AppState>,
    auth: Auth,
    Query(filter): Query<Filter>,
    mut multipart: Multipart,
) -> Result<Json<Value>> {
    auth.scope("drafts:write")?;
    let brand = filter
        .brand_id
        .ok_or_else(|| ApiError::bad("brand_id is required"))?;
    auth.brand(brand)?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM brands WHERE id=$1)")
        .bind(brand)
        .fetch_one(&state.db)
        .await?;
    if !exists {
        return Err(ApiError::missing());
    }
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| ApiError::bad("Invalid multipart upload"))?
    {
        if field.name() != Some("file") {
            continue;
        }
        let name = field
            .file_name()
            .unwrap_or("upload")
            .chars()
            .filter(|c| !c.is_control() && *c != '/' && *c != '\\')
            .take(200)
            .collect::<String>();
        let bytes = field
            .bytes()
            .await
            .map_err(|_| ApiError::bad("Upload exceeds the size limit or is incomplete"))?;
        if bytes.is_empty() || bytes.len() > MAX_SIZE {
            return Err(ApiError::bad("Upload must be between 1 byte and 25 MiB"));
        }
        let mime = detect(&bytes).ok_or_else(|| {
            ApiError::bad("Only JPEG, PNG, GIF, WebP, MP4 and WebM are supported")
        })?;
        let id = Uuid::new_v4();
        let key = format!("{id}.bin");
        let path = state.config.media_dir.join(&key);
        tokio::fs::write(&path, &bytes)
            .await
            .map_err(ApiError::internal)?;
        let result:std::result::Result<Value,sqlx::Error>=sqlx::query_scalar("INSERT INTO media(id,brand_id,name,mime,size,storage_key) VALUES($1,$2,$3,$4,$5,$6) RETURNING (to_jsonb(media.*)-'storage_key')||jsonb_build_object('url','/api/v1/media/'||id||'/file')").bind(id).bind(brand).bind(name).bind(mime).bind(bytes.len() as i64).bind(key).fetch_one(&state.db).await;
        match result {
            Ok(row) => return Ok(data(row)),
            Err(e) => {
                let _ = tokio::fs::remove_file(path).await;
                return Err(e.into());
            }
        }
    }
    Err(ApiError::bad("Multipart field 'file' is required"))
}
pub async fn file(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Response> {
    let row = sqlx::query("SELECT brand_id,mime,storage_key FROM media WHERE id=$1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    auth.brand(row.get("brand_id"))?;
    let bytes = tokio::fs::read(
        state
            .config
            .media_dir
            .join(row.get::<String, _>("storage_key")),
    )
    .await
    .map_err(ApiError::internal)?;
    let mime: String = row.get("mime");
    let mut response = (StatusCode::OK, Body::from(bytes)).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        mime.parse().map_err(ApiError::internal)?,
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        "private, max-age=300".parse().unwrap(),
    );
    response
        .headers_mut()
        .insert("x-content-type-options", "nosniff".parse().unwrap());
    Ok(response)
}
pub async fn delete(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    auth.scope("drafts:write")?;
    let mut tx = state.db.begin().await?;
    let row = sqlx::query("SELECT brand_id,storage_key FROM media WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    auth.brand(row.get("brand_id"))?;
    let referenced: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM media_references WHERE media_id=$1)")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
    if referenced {
        return Err(ApiError::conflict(
            "Media is referenced by a draft or publication",
        ));
    }
    let key: String = row.get("storage_key");
    sqlx::query("DELETE FROM media WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    if let Err(e) = tokio::fs::remove_file(state.config.media_dir.join(key)).await {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::error!(media_id=%id,error=%e,"orphan media file requires cleanup");
        }
    }
    Ok(data(json!({"ok":true})))
}
