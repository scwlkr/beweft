use crate::{
    auth::Auth,
    catalog::Filter,
    error::{ApiError, Result, data},
    listing::Page,
    state::AppState,
};
use axum::{
    Json, RequestExt,
    body::{Body, BodyDataStream},
    extract::{FromRequest, Multipart, Path, Query, Request, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use futures_util::{StreamExt, stream::Fuse};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use std::{path::PathBuf, sync::Arc};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
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
#[derive(Deserialize, Default)]
pub struct MediaListFilter {
    brand_id: Option<Uuid>,
    limit: Option<u32>,
    cursor: Option<String>,
}
pub async fn list(
    State(state): State<AppState>,
    auth: Auth,
    Query(filter): Query<MediaListFilter>,
) -> Result<Json<Value>> {
    if let Some(id) = filter.brand_id {
        auth.brand(id)?;
    }
    let page = Page::new(
        "media",
        filter.limit,
        filter.cursor.as_deref(),
        json!({"brand_id":filter.brand_id}),
        &auth,
    )?;
    if let Some(page) = page {
        let rows = sqlx::query_as("SELECT (to_jsonb(m)-'storage_key')||jsonb_build_object('url','/api/v1/media/'||m.id||'/file'),m.created_at,m.id FROM media m WHERE ($1::uuid IS NULL OR m.brand_id=$1) AND (cardinality($2::uuid[])=0 OR m.brand_id=ANY($2)) AND ($3::timestamptz IS NULL OR (m.created_at,m.id)<($3,$4)) ORDER BY m.created_at DESC,m.id DESC LIMIT $5").bind(filter.brand_id).bind(&auth.brand_ids).bind(page.created_at).bind(page.id).bind(page.limit + 1).fetch_all(&state.db).await?;
        page.finish(rows)
    } else {
        let rows:Vec<Value>=sqlx::query_scalar("SELECT (to_jsonb(m)-'storage_key')||jsonb_build_object('url','/api/v1/media/'||id||'/file') FROM media m WHERE ($1::uuid IS NULL OR brand_id=$1) AND (cardinality($2::uuid[])=0 OR brand_id=ANY($2)) ORDER BY created_at DESC").bind(filter.brand_id).bind(&auth.brand_ids).fetch_all(&state.db).await?;
        Ok(data(rows))
    }
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
pub async fn get(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let (row, brand): (Value, Uuid) = sqlx::query_as("SELECT (to_jsonb(m)-'storage_key')||jsonb_build_object('url','/api/v1/media/'||m.id||'/file'),m.brand_id FROM media m WHERE m.id=$1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    auth.brand(brand)?;
    Ok(data(row))
}
pub async fn upload(
    State(state): State<AppState>,
    auth: Auth,
    Query(filter): Query<Filter>,
    headers: HeaderMap,
    request: Request,
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
    // Multipart stops at its closing boundary, which can precede the end of
    // the HTTP body. Retain the limited stream so its remaining epilogue can
    // be drained before accepting or returning a known validation rejection.
    let (parts, body) = request.with_limited_body().into_parts();
    let shared = Arc::new(Mutex::new(body.into_data_stream().fuse()));
    let parser_body = futures_util::stream::unfold(shared.clone(), |shared| async move {
        let chunk = shared.lock().await.next().await;
        chunk.map(|chunk| (chunk, shared))
    });
    let request = Request::from_parts(parts, Body::from_stream(parser_body));
    let multipart = Multipart::from_request(request, &state)
        .await
        .map_err(|_| ApiError::bad("Invalid multipart upload"))?;
    tokio::time::timeout(
        std::time::Duration::from_secs(900),
        receive_upload(&state, brand, reserved, multipart, shared),
    )
    .await
    .map_err(|_| ApiError::bad("Upload timed out after 15 minutes"))?
}
type SharedUploadBody = Arc<Mutex<Fuse<BodyDataStream>>>;

struct PendingUpload {
    id: Uuid,
    key: String,
    name: String,
    temporary: PathBuf,
    file: tokio::fs::File,
    guard: crate::media_storage::UploadGuard,
    size: u64,
    prefix: Vec<u8>,
}

async fn receive_upload(
    state: &AppState,
    brand: Uuid,
    reserved: u64,
    mut multipart: Multipart,
    shared: SharedUploadBody,
) -> Result<Json<Value>> {
    let mut pending: Option<PendingUpload> = None;
    let mut extra_fields = false;
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|_| ApiError::bad("Invalid multipart upload"))?
    {
        if field.name() != Some("file") || pending.is_some() || extra_fields {
            extra_fields = true;
            while field
                .chunk()
                .await
                .map_err(|_| ApiError::bad("Upload exceeds the size limit or is incomplete"))?
                .is_some()
            {}
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
        let guard = crate::media_storage::reserve(state, id, reserved).await?;
        let temporary = state.config.media_dir.join(format!(".upload-{id}.part"));
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .await
            .map_err(ApiError::internal)?;
        let mut size = 0u64;
        let mut prefix = Vec::with_capacity(12);
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
            let sample = (12 - prefix.len()).min(chunk.len());
            prefix.extend_from_slice(&chunk[..sample]);
            file.write_all(&chunk).await.map_err(ApiError::internal)?;
        }
        pending = Some(PendingUpload {
            id,
            key,
            name,
            temporary,
            file,
            guard,
            size,
            prefix,
        });
    }
    // Drop the parser first: it may hold an outstanding poll on the shared
    // body. Data already buffered by it has already passed the body limit.
    drop(multipart);
    while let Some(chunk) = shared.lock().await.next().await {
        chunk.map_err(|_| ApiError::bad("Upload exceeds the size limit or is incomplete"))?;
    }
    if extra_fields {
        return Err(ApiError::bad(
            "Exactly one multipart field named 'file' is required",
        ));
    }
    let Some(PendingUpload {
        id,
        key,
        name,
        temporary,
        file,
        mut guard,
        size,
        prefix,
    }) = pending
    else {
        return Err(ApiError::bad("Multipart field 'file' is required"));
    };
    if size == 0 {
        return Err(ApiError::bad("Upload must not be empty"));
    }
    let mime = detect(&prefix).ok_or_else(|| {
        ApiError::bad_code(
            "unsupported_media_type",
            "Only JPEG, PNG, GIF, WebP, MP4 and WebM are supported",
        )
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
    Ok(data(row))
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
    if body.alt_text.contains('\0') {
        return Err(ApiError::bad_code(
            "media_description_contains_nul",
            "Media descriptions cannot contain NUL characters",
        ));
    }
    if body.alt_text.chars().count() > 2000 {
        return Err(ApiError::bad("Alt text must be at most 2,000 characters"));
    }
    if body.metadata_version < 1 {
        return Err(ApiError::bad("Invalid media metadata version"));
    }
    let brand: Uuid = sqlx::query_scalar("SELECT brand_id FROM media WHERE id=$1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    auth.brand(brand)?;
    let row: Option<Value> = sqlx::query_scalar("UPDATE media SET alt_text=$2,metadata_version=metadata_version+1 WHERE id=$1 AND metadata_version=$3 RETURNING (to_jsonb(media.*)-'storage_key')||jsonb_build_object('url','/api/v1/media/'||id||'/file')")
        .bind(id)
        .bind(body.alt_text)
        .bind(body.metadata_version)
        .fetch_optional(&state.db)
        .await?;
    let row = row.ok_or_else(|| {
        ApiError::conflict_code(
            "media_version_changed",
            "Media description changed. Refresh before editing.",
        )
    })?;
    Ok(data(row))
}
