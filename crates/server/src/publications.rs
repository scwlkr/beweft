use crate::{
    auth::Auth,
    catalog::Filter,
    error::{ApiError, Result, data, required, uuid},
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, Query, State},
};
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use uuid::Uuid;
const SELECT: &str = "SELECT (to_jsonb(p)-'lease_until')||jsonb_build_object('channel_name',c.name,'provider',c.provider) FROM publications p JOIN channels c ON c.id=p.channel_id";
pub async fn get_publication(db: &PgPool, id: Uuid) -> Result<Value> {
    Ok(sqlx::query_scalar(&format!("{SELECT} WHERE p.id=$1"))
        .bind(id)
        .fetch_one(db)
        .await?)
}
pub async fn list(
    State(state): State<AppState>,
    auth: Auth,
    Query(filter): Query<Filter>,
) -> Result<Json<Value>> {
    if let Some(id) = filter.brand_id {
        auth.brand(id)?;
    }
    if let Some(id) = filter.channel_id {
        auth.brand(crate::catalog::channel_brand(&state, id).await?)?;
    }
    let rows:Vec<Value>=sqlx::query_scalar(&format!("{SELECT} WHERE ($1::uuid IS NULL OR p.brand_id=$1) AND ($2::uuid IS NULL OR p.channel_id=$2) AND (cardinality($3::uuid[])=0 OR p.brand_id=ANY($3)) ORDER BY p.scheduled_at,p.created_at")).bind(filter.brand_id).bind(filter.channel_id).bind(&auth.brand_ids).fetch_all(&state.db).await?;
    Ok(data(rows))
}
pub async fn attempts(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let p = get_publication(&state.db, id).await?;
    auth.brand(uuid(&p, "brand_id")?)?;
    let rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(a) FROM publication_attempts a WHERE publication_id=$1 ORDER BY started_at DESC").bind(id).fetch_all(&state.db).await?;
    Ok(data(rows))
}
pub async fn cancel(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    change(&state, &auth, id, "cancel", json!({})).await
}
pub async fn retry(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    change(&state, &auth, id, "retry", json!({})).await
}
pub async fn manual_complete(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(body): Json<Value>,
) -> Result<Json<Value>> {
    change(&state, &auth, id, "manual_complete", body).await
}
pub async fn reconcile(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(body): Json<Value>,
) -> Result<Json<Value>> {
    change(&state, &auth, id, "reconcile", body).await
}
pub async fn reschedule(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(body): Json<Value>,
) -> Result<Json<Value>> {
    change(&state, &auth, id, "reschedule", body).await
}
async fn change(
    state: &AppState,
    auth: &Auth,
    id: Uuid,
    action: &str,
    body: Value,
) -> Result<Json<Value>> {
    auth.scope("publishing:write")?;
    let mut tx = state.db.begin().await?;
    let row=sqlx::query("SELECT p.*,c.provider FROM publications p JOIN channels c ON c.id=p.channel_id WHERE p.id=$1 FOR UPDATE OF p").bind(id).fetch_one(&mut *tx).await?;
    auth.brand(row.get("brand_id"))?;
    let current: String = row.get("state");
    let provider: String = row.get("provider");
    let mut scheduled_at = row.get::<chrono::DateTime<Utc>, _>("scheduled_at");
    let target = match action {
        "cancel" if ["queued", "manual_required", "failed"].contains(&current.as_str()) => {
            "canceled"
        }
        "retry" if current == "failed" => {
            scheduled_at = Utc::now();
            "queued"
        }
        "manual_complete" if current == "manual_required" && provider == "manual" => "published",
        "reconcile" if current == "uncertain" => {
            required(&body, "note")?;
            match body["resolution"].as_str() {
                Some("published") => "published",
                Some("not_published") => "failed",
                _ => {
                    return Err(ApiError::bad(
                        "resolution must be published or not_published",
                    ));
                }
            }
        }
        "reschedule" if ["queued", "manual_required"].contains(&current.as_str()) => {
            scheduled_at = crate::schedules::parse_future(&body["scheduled_at"])?;
            "queued"
        }
        _ => {
            return Err(ApiError::conflict(format!(
                "Cannot {action} a {current} publication"
            )));
        }
    };
    let external_url = body["external_url"].as_str();
    if let Some(url) = external_url {
        let parsed = url::Url::parse(url).map_err(|_| ApiError::bad("Invalid external URL"))?;
        if !matches!(parsed.scheme(), "https" | "http")
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err(ApiError::bad(
                "External URL must use HTTP(S) and contain no credentials",
            ));
        }
    }
    sqlx::query("UPDATE publications SET state=$2,scheduled_at=$3,external_id=COALESCE($4,external_id),external_url=COALESCE($5,external_url),last_error=CASE WHEN $2 IN ('queued','published','canceled') THEN NULL ELSE last_error END,lease_until=NULL,updated_at=now() WHERE id=$1").bind(id).bind(target).bind(scheduled_at).bind(body["external_id"].as_str()).bind(external_url).execute(&mut *tx).await?;
    sqlx::query(
        "INSERT INTO audit_events(id,owner_id,action,subject_id,detail) VALUES($1,$2,$3,$4,$5)",
    )
    .bind(Uuid::new_v4())
    .bind(auth.owner_id)
    .bind(action)
    .bind(id)
    .bind(&body)
    .execute(&mut *tx)
    .await?;
    if ["manual_complete", "reconcile"].contains(&action) {
        sqlx::query("INSERT INTO publication_attempts(id,publication_id,state,provider,finished_at,receipt) VALUES($1,$2,$3,$4,now(),$5)").bind(Uuid::new_v4()).bind(id).bind(target).bind(provider).bind(json!({"verification":"owner_attestation","action":action,"detail":body})).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(data(get_publication(&state.db, id).await?))
}
pub async fn reorder(
    State(state): State<AppState>,
    auth: Auth,
    Json(body): Json<Value>,
) -> Result<Json<Value>> {
    auth.scope("publishing:write")?;
    let channel = uuid(&body, "channel_id")?;
    auth.brand(crate::catalog::channel_brand(&state, channel).await?)?;
    let ids: Vec<Uuid> = serde_json::from_value(body["publication_ids"].clone())
        .map_err(|_| ApiError::bad("publication_ids must be UUIDs"))?;
    let mut tx = state.db.begin().await?;
    crate::schedules::lock_channel(&mut tx, channel).await?;
    let rows=sqlx::query("SELECT id,scheduled_at FROM publications WHERE channel_id=$1 AND state='queued' ORDER BY scheduled_at,id FOR UPDATE").bind(channel).fetch_all(&mut *tx).await?;
    let actual: std::collections::HashSet<Uuid> = rows.iter().map(|r| r.get("id")).collect();
    let requested: std::collections::HashSet<Uuid> = ids.iter().copied().collect();
    if actual != requested || ids.len() != requested.len() {
        return Err(ApiError::conflict(
            "Include every queued publication exactly once; reload the queue",
        ));
    }
    // Temporarily leave the partial unique index while holding every row lock; no observer sees this intermediate state.
    sqlx::query("UPDATE publications SET state='canceled' WHERE id=ANY($1)")
        .bind(&ids)
        .execute(&mut *tx)
        .await?;
    for (id, row) in ids.iter().zip(rows.iter()) {
        sqlx::query(
            "UPDATE publications SET state='queued',scheduled_at=$2,updated_at=now() WHERE id=$1",
        )
        .bind(id)
        .bind(row.get::<chrono::DateTime<Utc>, _>("scheduled_at"))
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    let mut result = Vec::new();
    for id in ids {
        result.push(get_publication(&state.db, id).await?);
    }
    Ok(data(result))
}
