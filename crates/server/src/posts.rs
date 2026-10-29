use crate::{
    auth::Auth,
    error::{ApiError, Result, data, uuid},
    listing::Page,
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use uuid::Uuid;
const POST_STATUS: &str = "CASE WHEN EXISTS(SELECT 1 FROM publications u WHERE u.post_id=p.id AND u.state IN ('failed','uncertain','manual_required')) THEN 'attention' WHEN EXISTS(SELECT 1 FROM publications u WHERE u.post_id=p.id AND u.state IN ('queued','sending')) THEN 'scheduled' WHEN EXISTS(SELECT 1 FROM publications u WHERE u.post_id=p.id AND u.state='published') THEN 'published' ELSE 'draft' END";
#[derive(Deserialize, Default)]
pub struct PostListFilter {
    brand_id: Option<Uuid>,
    limit: Option<u32>,
    cursor: Option<String>,
    status: Option<String>,
    view: Option<String>,
}
pub async fn get_post(db: &PgPool, id: Uuid) -> Result<Value> {
    Ok(sqlx::query_scalar(&format!(
        "SELECT to_jsonb(p)||jsonb_build_object('status',{POST_STATUS}) FROM posts p WHERE p.id=$1"
    ))
    .bind(id)
    .fetch_one(db)
    .await?)
}
pub async fn list(
    State(state): State<AppState>,
    auth: Auth,
    Query(filter): Query<PostListFilter>,
) -> Result<Json<Value>> {
    if let Some(id) = filter.brand_id {
        auth.brand(id)?;
    }
    if filter.status.as_deref().is_some_and(|s| s != "draft")
        || filter.view.as_deref().is_some_and(|v| v != "summary")
    {
        return Err(ApiError::bad("Unsupported post status or view"));
    }
    let summary = filter.view.as_deref() == Some("summary");
    let only_drafts = filter.status.as_deref() == Some("draft");
    let value = if summary {
        format!(
            "jsonb_build_object('id',p.id,'brand_id',p.brand_id,'title',p.title,'text',p.text,'media_ids',p.media_ids,'version',p.version,'status',{POST_STATUS},'created_at',p.created_at,'updated_at',p.updated_at)"
        )
    } else {
        format!("to_jsonb(p)||jsonb_build_object('status',{POST_STATUS})")
    };
    let filters = json!({"brand_id":filter.brand_id,"status":filter.status,"view":filter.view});
    let page = Page::new(
        "posts",
        filter.limit,
        filter.cursor.as_deref(),
        filters,
        &auth,
    )?;
    let condition = format!(
        "($1::uuid IS NULL OR p.brand_id=$1) AND (cardinality($2::uuid[])=0 OR p.brand_id=ANY($2)) AND (NOT $3::boolean OR ({POST_STATUS})='draft')"
    );
    if let Some(page) = page {
        let query = format!(
            "SELECT {value},p.created_at,p.id FROM posts p WHERE {condition} AND ($4::timestamptz IS NULL OR (p.created_at,p.id)<($4,$5)) ORDER BY p.created_at DESC,p.id DESC LIMIT $6"
        );
        let rows = sqlx::query_as(&query)
            .bind(filter.brand_id)
            .bind(&auth.brand_ids)
            .bind(only_drafts)
            .bind(page.created_at)
            .bind(page.id)
            .bind(page.limit + 1)
            .fetch_all(&state.db)
            .await?;
        page.finish(rows)
    } else {
        let query =
            format!("SELECT {value} FROM posts p WHERE {condition} ORDER BY p.updated_at DESC");
        let rows: Vec<Value> = sqlx::query_scalar(&query)
            .bind(filter.brand_id)
            .bind(&auth.brand_ids)
            .bind(only_drafts)
            .fetch_all(&state.db)
            .await?;
        Ok(data(rows))
    }
}
pub async fn get(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let post = get_post(&state.db, id).await?;
    auth.brand(uuid(&post, "brand_id")?)?;
    Ok(data(post))
}
pub async fn validate_content(db: &PgPool, brand: Uuid, body: &Value) -> Result<()> {
    validate_options(body)?;
    if body["title"]
        .as_str()
        .is_some_and(|s| s.chars().count() > 1000)
    {
        return Err(ApiError::bad("Title must be at most 1,000 characters"));
    }
    for key in ["title", "text"] {
        if body.get(key).is_some_and(|v| !v.is_string()) {
            return Err(ApiError::bad(format!("{key} must be a string")));
        }
    }

    if body["text"].as_str().unwrap_or("").len() > 100_000 {
        return Err(ApiError::bad_code(
            "post_text_too_long",
            "Text must be at most 100,000 bytes",
        ));
    }
    let media: Vec<Uuid> =
        serde_json::from_value(body.get("media_ids").cloned().unwrap_or(json!([])))
            .map_err(|_| ApiError::bad("media_ids must be an array of UUIDs"))?;
    check_media(db, brand, &media).await?;
    let variants = body.get("variants").cloned().unwrap_or(json!([]));
    let variants = variants
        .as_array()
        .ok_or_else(|| ApiError::bad("variants must be an array"))?;
    if variants.len() > 50 {
        return Err(ApiError::bad("At most 50 variants are allowed"));
    }
    let mut seen = std::collections::HashSet::new();
    for variant in variants {
        validate_options(variant)?;
        let channel = uuid(variant, "channel_id")?;
        if !seen.insert(channel) {
            return Err(ApiError::bad("Only one variant per channel is allowed"));
        }
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM channels WHERE id=$1 AND brand_id=$2)")
                .bind(channel)
                .bind(brand)
                .fetch_one(db)
                .await?;
        if !exists {
            return Err(ApiError::bad(
                "Variant channel must belong to the same brand",
            ));
        }
        if variant["text"].as_str().is_some_and(|s| s.len() > 100_000) {
            return Err(ApiError::bad_code(
                "post_text_too_long",
                "Variant text must be at most 100,000 bytes",
            ));
        }
        if let Some(ids) = variant.get("media_ids") {
            let ids: Vec<Uuid> = serde_json::from_value(ids.clone())
                .map_err(|_| ApiError::bad("Variant media_ids must be UUIDs"))?;
            check_media(db, brand, &ids).await?;
        }
    }
    Ok(())
}
async fn check_media(db: &PgPool, brand: Uuid, ids: &[Uuid]) -> Result<()> {
    if ids.len() > 20 {
        return Err(ApiError::bad("At most 20 media items are allowed"));
    }
    for id in ids {
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM media WHERE id=$1 AND brand_id=$2)")
                .bind(id)
                .bind(brand)
                .fetch_one(db)
                .await?;
        if !exists {
            return Err(ApiError::bad("Media must belong to the same brand"));
        }
    }
    Ok(())
}
pub async fn create(
    State(state): State<AppState>,
    auth: Auth,
    Json(input): Json<crate::contracts::CreatePostRequest>,
) -> Result<Json<Value>> {
    let body = serde_json::to_value(input).map_err(ApiError::internal)?;
    auth.scope("drafts:write")?;
    let brand = uuid(&body, "brand_id")?;
    auth.brand(brand)?;
    validate_content(&state.db, brand, &body).await?;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO posts(id,brand_id,title,text,media_ids,variants,options) VALUES($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(id)
    .bind(brand)
    .bind(body["title"].as_str().unwrap_or(""))
    .bind(body["text"].as_str().unwrap_or(""))
    .bind(body.get("media_ids").cloned().unwrap_or(json!([])))
    .bind(body.get("variants").cloned().unwrap_or(json!([])))
    .bind(body.get("options").cloned().unwrap_or(json!({})))
    .execute(&state.db)
    .await?;
    Ok(data(get_post(&state.db, id).await?))
}
pub async fn update(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(input): Json<crate::contracts::UpdatePostRequest>,
) -> Result<Json<Value>> {
    let body = serde_json::to_value(input).map_err(ApiError::internal)?;
    auth.scope("drafts:write")?;
    let existing = get_post(&state.db, id).await?;
    let brand = uuid(&existing, "brand_id")?;
    auth.brand(brand)?;
    let version = body["version"]
        .as_i64()
        .ok_or_else(|| ApiError::bad("version is required"))?;
    let mut merged = existing;
    for key in ["title", "text", "media_ids", "variants", "options"] {
        if let Some(value) = body.get(key) {
            merged[key] = value.clone();
        }
    }
    validate_content(&state.db, brand, &merged).await?;
    let updated=sqlx::query("UPDATE posts SET title=$3,text=$4,media_ids=$5,variants=$6,options=$7,version=version+1,updated_at=now() WHERE id=$1 AND version=$2").bind(id).bind(i32::try_from(version).map_err(|_|ApiError::bad("Invalid version"))?).bind(merged["title"].as_str().unwrap_or("")).bind(merged["text"].as_str().unwrap_or("")).bind(&merged["media_ids"]).bind(&merged["variants"]).bind(&merged["options"]).execute(&state.db).await?;
    if updated.rows_affected() == 0 {
        return Err(ApiError::conflict_code(
            "post_version_conflict",
            "Draft changed; reload before saving",
        ));
    }
    Ok(data(get_post(&state.db, id).await?))
}
pub async fn delete(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    auth.scope("drafts:write")?;
    let mut tx = state.db.begin().await?;
    let row = sqlx::query("SELECT brand_id FROM posts WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    auth.brand(row.get("brand_id"))?;
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM publications WHERE post_id=$1)")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
    if exists {
        return Err(ApiError::conflict(
            "Posts with publication history cannot be deleted",
        ));
    }
    sqlx::query("DELETE FROM posts WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(data(json!({"ok":true})))
}

fn validate_options(body: &Value) -> Result<()> {
    if let Some(options) = body.get("options")
        && (!options.is_object() || options.to_string().len() > 32768)
    {
        return Err(ApiError::bad("options must be a JSON object within 32 KiB"));
    }
    Ok(())
}
