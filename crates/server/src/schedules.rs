use crate::{
    auth::Auth,
    catalog::{Filter, channel_brand, validate_timezone},
    error::{ApiError, Result, data, required, uuid},
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, Query, State},
};
use chrono::{DateTime, Datelike, Duration, NaiveTime, TimeZone, Utc};
use serde_json::{Value, json};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;
pub async fn list(
    State(state): State<AppState>,
    auth: Auth,
    Query(filter): Query<Filter>,
) -> Result<Json<Value>> {
    let channel = filter
        .channel_id
        .ok_or_else(|| ApiError::bad("channel_id is required"))?;
    auth.brand(channel_brand(&state, channel).await?)?;
    let rows:Vec<Value>=sqlx::query_scalar("SELECT (to_jsonb(s)-'local_time')||jsonb_build_object('time',local_time) FROM schedule_rules s WHERE channel_id=$1 ORDER BY local_time").bind(channel).fetch_all(&state.db).await?;
    Ok(data(rows))
}
pub async fn create(
    State(state): State<AppState>,
    auth: Auth,
    Json(input): Json<crate::contracts::CreateScheduleRequest>,
) -> Result<Json<Value>> {
    let body = serde_json::to_value(input).map_err(ApiError::internal)?;
    auth.scope("publishing:write")?;
    let channel = uuid(&body, "channel_id")?;
    auth.brand(channel_brand(&state, channel).await?)?;
    let days: Vec<u32> = serde_json::from_value(body["days"].clone())
        .map_err(|_| ApiError::bad("days must be an array of weekdays, Monday=1"))?;
    if days.is_empty() || days.len() > 7 || days.iter().any(|d| !(1..=7).contains(d)) {
        return Err(ApiError::bad("Select between one and seven weekdays"));
    }
    let time = required(&body, "time")?;
    NaiveTime::parse_from_str(&time, "%H:%M").map_err(|_| ApiError::bad("time must be HH:MM"))?;
    let timezone = required(&body, "timezone")?;
    validate_timezone(&timezone)?;
    let row:Value=sqlx::query_scalar("INSERT INTO schedule_rules(id,channel_id,days,local_time,timezone) VALUES($1,$2,$3,$4,$5) RETURNING (to_jsonb(schedule_rules.*)-'local_time')||jsonb_build_object('time',local_time)").bind(Uuid::new_v4()).bind(channel).bind(json!(days)).bind(time).bind(timezone).fetch_one(&state.db).await?;
    Ok(data(row))
}
pub async fn delete(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    auth.scope("publishing:write")?;
    let brand:Uuid=sqlx::query_scalar("SELECT c.brand_id FROM schedule_rules s JOIN channels c ON c.id=s.channel_id WHERE s.id=$1").bind(id).fetch_one(&state.db).await?;
    auth.brand(brand)?;
    sqlx::query("DELETE FROM schedule_rules WHERE id=$1")
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(data(json!({"ok":true})))
}
pub async fn lock_channel(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> Result<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(id.to_string())
        .execute(&mut **tx)
        .await?;
    Ok(())
}
pub async fn next_slot(tx: &mut Transaction<'_, Postgres>, channel: Uuid) -> Result<DateTime<Utc>> {
    let rules =
        sqlx::query("SELECT days,local_time,timezone FROM schedule_rules WHERE channel_id=$1")
            .bind(channel)
            .fetch_all(&mut **tx)
            .await?;
    if rules.is_empty() {
        return Err(ApiError::bad(
            "This channel needs a posting schedule or an exact date",
        ));
    }
    let now = Utc::now();
    next_slot_after(tx, channel, rules, now).await
}
pub async fn next_slot_after(
    tx: &mut Transaction<'_, Postgres>,
    channel: Uuid,
    rules: Vec<sqlx::postgres::PgRow>,
    now: DateTime<Utc>,
) -> Result<DateTime<Utc>> {
    let mut candidates = Vec::new();
    for rule in rules {
        let tz = validate_timezone(&rule.get::<String, _>("timezone"))?;
        let time = NaiveTime::parse_from_str(&rule.get::<String, _>("local_time"), "%H:%M")
            .map_err(ApiError::internal)?;
        let days: Vec<u32> =
            serde_json::from_value(rule.get("days")).map_err(ApiError::internal)?;
        let local = now.with_timezone(&tz).date_naive();
        for offset in 0..60 {
            let date = local + Duration::days(offset);
            if !days.contains(&date.weekday().number_from_monday()) {
                continue;
            }
            if let Some(slot) = tz.from_local_datetime(&date.and_time(time)).earliest() {
                let slot = slot.with_timezone(&Utc);
                if slot > now {
                    candidates.push(slot);
                }
            }
        }
    }
    candidates.sort_unstable();
    candidates.dedup();
    for candidate in candidates {
        let occupied:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM publications WHERE channel_id=$1 AND scheduled_at=$2 AND state IN ('queued','sending','manual_required','uncertain'))").bind(channel).bind(candidate).fetch_one(&mut **tx).await?;
        if !occupied {
            return Ok(candidate);
        }
    }
    Err(ApiError::conflict(
        "No free posting slot in the next 60 days",
    ))
}
pub fn parse_future(value: &Value) -> Result<DateTime<Utc>> {
    let raw = value
        .as_str()
        .ok_or_else(|| ApiError::bad("scheduled_at must be an RFC3339 timestamp"))?;
    let time = DateTime::parse_from_rfc3339(raw)
        .map_err(|_| ApiError::bad("Invalid scheduled_at"))?
        .with_timezone(&Utc);
    if time < Utc::now() - Duration::seconds(5) {
        return Err(ApiError::bad("Choose a future posting time"));
    }
    Ok(time)
}
pub async fn schedule_post(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(input): Json<crate::contracts::SchedulePostRequest>,
) -> Result<Json<Value>> {
    let body = serde_json::to_value(input).map_err(ApiError::internal)?;
    auth.scope("publishing:write")?;
    let mut channels: Vec<Uuid> = serde_json::from_value(body["channel_ids"].clone())
        .map_err(|_| ApiError::bad("channel_ids must be UUIDs"))?;
    if channels.is_empty() || channels.len() > 50 {
        return Err(ApiError::bad("Select between one and 50 channels"));
    }
    channels.sort_unstable();
    let count = channels.len();
    channels.dedup();
    if count != channels.len() {
        return Err(ApiError::bad("Duplicate channel IDs"));
    }
    let exact = body
        .get("scheduled_at")
        .filter(|v| !v.is_null())
        .map(parse_future)
        .transpose()?;
    let mut tx = state.db.begin().await?;
    let post = sqlx::query("SELECT * FROM posts WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    let brand: Uuid = post.get("brand_id");
    auth.brand(brand)?;
    let expected_version = body["version"]
        .as_i64()
        .ok_or_else(|| ApiError::bad("version is required"))?;
    if expected_version != i64::from(post.get::<i32, _>("version")) {
        return Err(ApiError::conflict(
            "Draft changed; reload before scheduling",
        ));
    }
    let variants: Value = post.get("variants");
    let mut ids = Vec::new();
    for channel in channels {
        lock_channel(&mut tx, channel).await?;
        let valid: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM channels WHERE id=$1 AND brand_id=$2)")
                .bind(channel)
                .bind(brand)
                .fetch_one(&mut *tx)
                .await?;
        if !valid {
            return Err(ApiError::bad(
                "Every channel must belong to the post's brand",
            ));
        }
        let scheduled_at = match exact {
            Some(t) => t,
            None => next_slot(&mut tx, channel).await?,
        };
        let variant = variants.as_array().and_then(|v| {
            v.iter()
                .find(|v| v["channel_id"].as_str() == Some(&channel.to_string()))
        });
        let text = variant
            .and_then(|v| v["text"].as_str())
            .unwrap_or(post.get("text"));
        let media: Value = variant
            .and_then(|v| v.get("media_ids"))
            .cloned()
            .unwrap_or(post.get("media_ids"));
        if text.trim().is_empty() && media.as_array().is_none_or(|m| m.is_empty()) {
            return Err(ApiError::bad("A scheduled post needs text or media"));
        }
        let options = variant
            .and_then(|v| v.get("options"))
            .cloned()
            .unwrap_or(post.get("options"));
        let payload = json!({"title":post.get::<String,_>("title"),"text":text,"media_ids":media,"options":options});
        let publication = Uuid::new_v4();
        sqlx::query("INSERT INTO publications(id,post_id,brand_id,channel_id,post_version,scheduled_at,state,payload,scheduling_mode) VALUES($1,$2,$3,$4,$5,$6,'queued',$7,$8)").bind(publication).bind(id).bind(brand).bind(channel).bind(post.get::<i32,_>("version")).bind(scheduled_at).bind(payload).bind(if exact.is_some() { "exact" } else { "queue" }).execute(&mut *tx).await?;
        ids.push(publication);
    }
    tx.commit().await?;
    let mut rows = Vec::new();
    for id in ids {
        rows.push(crate::publications::get_publication(&state.db, id).await?);
    }
    Ok(data(rows))
}
