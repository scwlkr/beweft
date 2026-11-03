use crate::{
    auth::Auth,
    error::{ApiError, Result, data, required, uuid},
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Deserialize, Default)]
pub struct PageFilter {
    pub brand_id: Option<Uuid>,
}

fn validate_slug(slug: &str) -> Result<()> {
    if slug.is_empty() || slug.len() > 50 {
        return Err(ApiError::bad("Slug must be 1–50 characters"));
    }
    if !slug
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        return Err(ApiError::bad(
            "Slug may only contain lowercase letters, numbers, hyphens, and underscores",
        ));
    }
    Ok(())
}

fn validate_url(url: &str) -> Result<()> {
    if url.len() > 2000 {
        return Err(ApiError::bad("URL is too long"));
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(ApiError::bad("URL must start with http:// or https://"));
    }
    Ok(())
}

pub async fn list(
    State(state): State<AppState>,
    auth: Auth,
    Query(filter): Query<PageFilter>,
) -> Result<Json<Value>> {
    if let Some(id) = filter.brand_id {
        auth.brand(id)?;
    }
    let rows: Vec<Value> = sqlx::query_scalar(
        "SELECT to_jsonb(sp) || jsonb_build_object('links', COALESCE((SELECT jsonb_agg(to_jsonb(l) ORDER BY l.position, l.created_at) FROM start_page_links l WHERE l.start_page_id = sp.id), '[]'::jsonb)) FROM start_pages sp WHERE ($1::uuid IS NULL OR brand_id=$1) AND (cardinality($2::uuid[])=0 OR brand_id=ANY($2)) ORDER BY created_at",
    )
    .bind(filter.brand_id)
    .bind(&auth.brand_ids)
    .fetch_all(&state.db)
    .await?;
    Ok(data(rows))
}

pub async fn get_public(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<Value>> {
    let row: Option<Value> = sqlx::query_scalar(
        "SELECT to_jsonb(sp) || jsonb_build_object('links', COALESCE((SELECT jsonb_agg(to_jsonb(l) ORDER BY l.position, l.created_at) FROM start_page_links l WHERE l.start_page_id = sp.id AND l.enabled = true), '[]'::jsonb)) FROM start_pages sp WHERE sp.slug = $1 AND sp.published = true",
    )
    .bind(&slug)
    .fetch_optional(&state.db)
    .await?;
    let row = row.ok_or_else(|| ApiError::missing())?;
    Ok(data(row))
}

pub async fn create(
    State(state): State<AppState>,
    auth: Auth,
    Json(body): Json<Value>,
) -> Result<Json<Value>> {
    auth.scope("admin")?;
    let brand_id = uuid(&body, "brand_id")?;
    auth.brand(brand_id)?;
    let slug = required(&body, "slug")?.to_string();
    validate_slug(&slug)?;
    let title = body["title"].as_str().unwrap_or("").to_string();
    let bio = body["bio"].as_str().unwrap_or("").to_string();
    let theme = body["theme"].as_str().unwrap_or("classic").to_string();
    let published = body["published"].as_bool().unwrap_or(true);
    let avatar_media_id = body["avatar_media_id"]
        .as_str()
        .map(|s| Uuid::parse_str(s))
        .transpose()
        .map_err(|_| ApiError::bad("Invalid avatar_media_id"))?;
    let id = Uuid::new_v4();
    let row: Value = sqlx::query_scalar(
        "INSERT INTO start_pages(id,brand_id,slug,title,bio,theme,avatar_media_id,published) VALUES($1,$2,$3,$4,$5,$6,$7,$8) RETURNING to_jsonb(start_pages.*)",
    )
    .bind(id)
    .bind(brand_id)
    .bind(slug)
    .bind(title)
    .bind(bio)
    .bind(theme)
    .bind(avatar_media_id)
    .bind(published)
    .fetch_one(&state.db)
    .await
    .map_err(|e| {
        if let sqlx::Error::Database(db) = &e {
            if db.constraint() == Some("start_pages_slug_key") {
                return ApiError::bad("Slug already taken");
            }
            if db.constraint() == Some("start_pages_brand_id_key") {
                return ApiError::bad("Brand already has a start page");
            }
        }
        ApiError::internal("create failed")
    })?;
    Ok(data(row))
}

pub async fn update(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(body): Json<Value>,
) -> Result<Json<Value>> {
    auth.scope("admin")?;
    let brand_id: Uuid = sqlx::query_scalar("SELECT brand_id FROM start_pages WHERE id=$1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    auth.brand(brand_id)?;
    if let Some(slug) = body["slug"].as_str() {
        validate_slug(slug)?;
    }
    let slug = body["slug"].as_str().map(|s| s.to_string());
    let title = body["title"].as_str().map(|s| s.to_string());
    let bio = body["bio"].as_str().map(|s| s.to_string());
    let theme = body["theme"].as_str().map(|s| s.to_string());
    let published = body["published"].as_bool();
    let avatar_media_id = if body.get("avatar_media_id").is_some() {
        body["avatar_media_id"]
            .as_str()
            .map(|s| Uuid::parse_str(s))
            .transpose()
            .map_err(|_| ApiError::bad("Invalid avatar_media_id"))?
    } else {
        None
    };
    let row: Value = sqlx::query_scalar(
        "UPDATE start_pages SET slug=COALESCE($2,slug),title=COALESCE($3,title),bio=COALESCE($4,bio),theme=COALESCE($5,theme),published=COALESCE($6,published),avatar_media_id=CASE WHEN $7 THEN $8 ELSE avatar_media_id END,updated_at=now() WHERE id=$1 RETURNING to_jsonb(start_pages.*)",
    )
    .bind(id)
    .bind(slug)
    .bind(title)
    .bind(bio)
    .bind(theme)
    .bind(published)
    .bind(body.get("avatar_media_id").is_some())
    .bind(avatar_media_id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| {
        if let sqlx::Error::Database(db) = &e {
            if db.constraint() == Some("start_pages_slug_key") {
                return ApiError::bad("Slug already taken");
            }
        }
        ApiError::internal("update failed")
    })?;
    Ok(data(row))
}

pub async fn delete(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    auth.scope("admin")?;
    let brand_id: Uuid = sqlx::query_scalar("SELECT brand_id FROM start_pages WHERE id=$1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    auth.brand(brand_id)?;
    sqlx::query("DELETE FROM start_pages WHERE id=$1")
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(data(json!({"ok":true})))
}

pub async fn create_link(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(body): Json<Value>,
) -> Result<Json<Value>> {
    auth.scope("admin")?;
    let brand_id: Uuid = sqlx::query_scalar("SELECT brand_id FROM start_pages WHERE id=$1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    auth.brand(brand_id)?;
    let title = required(&body, "title")?.to_string();
    let url = required(&body, "url")?.to_string();
    validate_url(&url)?;
    if title.len() > 100 {
        return Err(ApiError::bad("Title must be at most 100 characters"));
    }
    let position: i32 = sqlx::query_scalar("SELECT COALESCE(MAX(position)+1,0) FROM start_page_links WHERE start_page_id=$1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    let enabled = body["enabled"].as_bool().unwrap_or(true);
    let row: Value = sqlx::query_scalar(
        "INSERT INTO start_page_links(id,start_page_id,title,url,position,enabled) VALUES($1,$2,$3,$4,$5,$6) RETURNING to_jsonb(start_page_links.*)",
    )
    .bind(Uuid::new_v4())
    .bind(id)
    .bind(title)
    .bind(url)
    .bind(position)
    .bind(enabled)
    .fetch_one(&state.db)
    .await?;
    Ok(data(row))
}

pub async fn update_link(
    State(state): State<AppState>,
    auth: Auth,
    Path((page_id, link_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<Value>,
) -> Result<Json<Value>> {
    auth.scope("admin")?;
    let brand_id: Uuid = sqlx::query_scalar("SELECT brand_id FROM start_pages WHERE id=$1")
        .bind(page_id)
        .fetch_one(&state.db)
        .await?;
    auth.brand(brand_id)?;
    if let Some(url) = body["url"].as_str() {
        validate_url(url)?;
    }
    if let Some(title) = body["title"].as_str() {
        if title.len() > 100 {
            return Err(ApiError::bad("Title must be at most 100 characters"));
        }
    }
    let title = body["title"].as_str();
    let url = body["url"].as_str();
    let enabled = body["enabled"].as_bool();
    let position = body["position"].as_i64().map(|v| v as i32);
    let row: Value = sqlx::query_scalar(
        "UPDATE start_page_links SET title=COALESCE($3,title),url=COALESCE($4,url),enabled=COALESCE($5,enabled),position=COALESCE($6,position) WHERE id=$2 AND start_page_id=$1 RETURNING to_jsonb(start_page_links.*)",
    )
    .bind(page_id)
    .bind(link_id)
    .bind(title)
    .bind(url)
    .bind(enabled)
    .bind(position)
    .fetch_one(&state.db)
    .await?;
    Ok(data(row))
}

pub async fn delete_link(
    State(state): State<AppState>,
    auth: Auth,
    Path((page_id, link_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>> {
    auth.scope("admin")?;
    let brand_id: Uuid = sqlx::query_scalar("SELECT brand_id FROM start_pages WHERE id=$1")
        .bind(page_id)
        .fetch_one(&state.db)
        .await?;
    auth.brand(brand_id)?;
    sqlx::query("DELETE FROM start_page_links WHERE id=$1 AND start_page_id=$2")
        .bind(link_id)
        .bind(page_id)
        .execute(&state.db)
        .await?;
    Ok(data(json!({"ok":true})))
}

pub async fn track_click(
    State(state): State<AppState>,
    Path((slug, link_id)): Path<(String, Uuid)>,
) -> Result<Json<Value>> {
    sqlx::query(
        "UPDATE start_page_links SET clicks = clicks + 1 WHERE id=$1 AND start_page_id IN (SELECT id FROM start_pages WHERE slug=$2 AND published=true)",
    )
    .bind(link_id)
    .bind(&slug)
    .execute(&state.db)
    .await?;
    Ok(data(json!({"ok":true})))
}
