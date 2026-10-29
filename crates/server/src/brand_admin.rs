use crate::{
    auth::Auth,
    contracts::UpdateBrandRequest,
    error::{ApiError, Result, data},
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;
#[derive(Deserialize, Default)]
pub struct BrandFilter {
    #[serde(default)]
    pub include_archived: bool,
}
pub async fn list(
    State(state): State<AppState>,
    auth: Auth,
    Query(filter): Query<BrandFilter>,
) -> Result<Json<Value>> {
    let rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(b) FROM brands b WHERE (cardinality($1::uuid[])=0 OR id=ANY($1)) AND ($2 OR archived_at IS NULL) ORDER BY created_at").bind(&auth.brand_ids).bind(filter.include_archived).fetch_all(&state.db).await?;
    Ok(data(rows))
}
pub async fn lock(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> Result<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("brand:{id}"))
        .execute(&mut **tx)
        .await?;
    Ok(())
}
pub async fn active(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> Result<()> {
    let archived: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT archived_at FROM brands WHERE id=$1")
            .bind(id)
            .fetch_one(&mut **tx)
            .await?;
    if archived.is_some() {
        return Err(ApiError::conflict(
            "This brand is archived. Restore it before scheduling.",
        ));
    }
    Ok(())
}
pub async fn update(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateBrandRequest>,
) -> Result<Json<Value>> {
    auth.scope("admin")?;
    auth.brand(id)?;
    if body
        .name
        .as_ref()
        .is_some_and(|s| s.trim().is_empty() || s.chars().count() > 100)
    {
        return Err(ApiError::bad("Brand name must contain 1–100 characters"));
    }
    if body.color.as_ref().is_some_and(|s| {
        s.len() != 7 || !s.starts_with('#') || !s[1..].chars().all(|c| c.is_ascii_hexdigit())
    }) {
        return Err(ApiError::bad("color must be a six-digit hex color"));
    }
    let mut tx = state.db.begin().await?;
    lock(&mut tx, id).await?;
    let channels: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM channels WHERE brand_id=$1 ORDER BY id")
            .bind(id)
            .fetch_all(&mut *tx)
            .await?;
    for channel in channels {
        crate::schedules::lock_channel(&mut tx, channel).await?;
    }
    if body.archived == Some(true) {
        let sending: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM publications WHERE brand_id=$1 AND state='sending')",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        if sending {
            return Err(ApiError::conflict(
                "A delivery is in flight. Wait for its outcome before archiving.",
            ));
        }
        sqlx::query("UPDATE channels SET paused=true WHERE brand_id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    let row:Value=sqlx::query_scalar("UPDATE brands SET name=COALESCE($2,name),color=COALESCE($3,color),archived_at=CASE WHEN $4::boolean IS NULL THEN archived_at WHEN $4 THEN COALESCE(archived_at,now()) ELSE NULL END WHERE id=$1 RETURNING to_jsonb(brands.*)").bind(id).bind(body.name.map(|s|s.trim().to_string())).bind(body.color).bind(body.archived).fetch_one(&mut *tx).await?;
    sqlx::query("INSERT INTO audit_events(id,owner_id,action,subject_id,detail) VALUES($1,$2,'brand_update',$3,$4)").bind(Uuid::new_v4()).bind(auth.owner_id).bind(id).bind(json!({"archived":body.archived})).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(data(row))
}
