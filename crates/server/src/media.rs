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
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use sqlx::Row;
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;
use uuid::Uuid;
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
pub async fn limits(State(state): State<AppState>, _auth: Auth) -> Result<Json<Value>> {
    let used: i64 = sqlx::query_scalar("SELECT COALESCE(sum(size),0)::bigint FROM media")
        .fetch_one(&state.db)
        .await?;
    let reserved:i64=sqlx::query_scalar("SELECT COALESCE(sum(reserved_bytes),0)::bigint FROM media_upload_reservations WHERE expires_at>now()").fetch_one(&state.db).await?;
    Ok(data(
        json!({"max_upload_bytes":state.config.max_upload_bytes,"quota_bytes":state.config.media_quota_bytes,"used_bytes":used,"reserved_bytes":reserved,"available_bytes":state.config.media_quota_bytes.saturating_sub((used+reserved) as u64)}),
    ))
}
pub async fn upload(
    State(state): State<AppState>,
    auth: Auth,
    Query(filter): Query<Filter>,
    headers: HeaderMap,
    multipart: Multipart,
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
    // Reserve a declared request length (an upper bound on the file), or the entire maximum for chunked requests.
    let reserved = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(state.config.max_upload_bytes)
        .min(state.config.max_upload_bytes);
    tokio::time::timeout(
        std::time::Duration::from_secs(900),
        receive_upload(&state, brand, reserved, multipart),
    )
    .await
    .map_err(|_| ApiError::bad("Upload timed out after 15 minutes"))?
}
async fn receive_upload(
    state: &AppState,
    brand: Uuid,
    reserved: u64,
    mut multipart: Multipart,
) -> Result<Json<Value>> {
    while let Some(mut field) = multipart
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
        let id = Uuid::new_v4();
        let key = format!("{id}.bin");
        let mut guard = crate::media_storage::reserve(state, id, reserved).await?;
        let temporary = state.config.media_dir.join(format!(".upload-{id}.part"));
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .await
            .map_err(ApiError::internal)?;
        let mut size = 0u64;
        let mut prefix = Vec::with_capacity(64);
        while let Some(chunk) = field
            .chunk()
            .await
            .map_err(|_| ApiError::bad("Upload exceeds the size limit or is incomplete"))?
        {
            size = size.saturating_add(chunk.len() as u64);
            if size > reserved {
                return Err(ApiError::bad(format!(
                    "Upload exceeds its declared length or the {}-byte limit",
                    state.config.max_upload_bytes
                )));
            }
            let sample = (64 - prefix.len()).min(chunk.len());
            prefix.extend_from_slice(&chunk[..sample]);
            if prefix.len() == 64 && detect(&prefix).is_none() {
                return Err(ApiError::bad(
                    "Only JPEG, PNG, GIF, WebP, MP4 and WebM are supported",
                ));
            }
            file.write_all(&chunk).await.map_err(ApiError::internal)?;
        }
        if size == 0 {
            return Err(ApiError::bad("Upload must not be empty"));
        }
        let mime = detect(&prefix).ok_or_else(|| {
            ApiError::bad("Only JPEG, PNG, GIF, WebP, MP4 and WebM are supported")
        })?;
        file.sync_all().await.map_err(ApiError::internal)?;
        drop(file);
        let mut tx = state.db.begin().await?;
        crate::media_storage::lock(&mut tx).await?;
        tokio::fs::rename(&temporary, state.config.media_dir.join(&key))
            .await
            .map_err(ApiError::internal)?;
        let row:Value=sqlx::query_scalar("INSERT INTO media(id,brand_id,name,mime,size,storage_key) VALUES($1,$2,$3,$4,$5,$6) RETURNING (to_jsonb(media.*)-'storage_key')||jsonb_build_object('url','/api/v1/media/'||id||'/file')").bind(id).bind(brand).bind(name).bind(mime).bind(size as i64).bind(key).fetch_one(&mut *tx).await?;
        sqlx::query("DELETE FROM media_upload_reservations WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        guard.finish();
        return Ok(data(row));
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
    let file = tokio::fs::File::open(
        state
            .config
            .media_dir
            .join(row.get::<String, _>("storage_key")),
    )
    .await
    .map_err(ApiError::internal)?;
    let size = file.metadata().await.map_err(ApiError::internal)?.len();
    let mime: String = row.get("mime");
    let mut response = (StatusCode::OK, Body::from_stream(ReaderStream::new(file))).into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_LENGTH, size.into());
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
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM media_references WHERE media_id=$1 UNION ALL SELECT 1 FROM template_media_references WHERE media_id=$1)")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
    if referenced {
        return Err(ApiError::conflict(
            "Media is referenced by a draft, publication or template",
        ));
    }
    let key: String = row.get("storage_key");
    sqlx::query("DELETE FROM media WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    if let Err(e) = tokio::fs::remove_file(state.config.media_dir.join(key)).await
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::error!(media_id=%id,error=%e,"orphan media file requires cleanup");
    }
    Ok(data(json!({"ok":true})))
}

pub async fn update(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(body): Json<crate::contracts::UpdateMediaRequest>,
) -> Result<Json<Value>> {
    auth.scope("drafts:write")?;
    if body.alt_text.chars().count() > 2000 {
        return Err(ApiError::bad("Alt text must be at most 2,000 characters"));
    }
    let brand: Uuid = sqlx::query_scalar("SELECT brand_id FROM media WHERE id=$1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    auth.brand(brand)?;
    let row:Value=sqlx::query_scalar("UPDATE media SET alt_text=$2 WHERE id=$1 RETURNING (to_jsonb(media.*)-'storage_key')||jsonb_build_object('url','/api/v1/media/'||id||'/file')").bind(id).bind(body.alt_text).fetch_one(&state.db).await?;
    Ok(data(row))
}
