use crate::error::{ApiError, Result};
use serde_json::{Value, json};
use sqlx::{Postgres, Row, Transaction, postgres::PgRow};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use uuid::Uuid;

fn variant_for<'a>(variants: &'a Value, channel: &str) -> Option<&'a Value> {
    variants
        .as_array()?
        .iter()
        .find(|v| v["channel_id"].as_str() == Some(channel))
}

fn effective_media(post: &PgRow, variant: Option<&Value>) -> Result<(Value, Vec<Uuid>)> {
    let media: Value = variant
        .and_then(|v| v.get("media_ids"))
        .cloned()
        .unwrap_or(post.get("media_ids"));
    let ids = serde_json::from_value(media.clone()).map_err(ApiError::internal)?;
    Ok((media, ids))
}

/// Lock the complete effective media set in a stable order before writing any
/// snapshots. A supplied map must cover exactly this set and its current versions.
pub async fn lock_effective_media(
    tx: &mut Transaction<'_, Postgres>,
    post: &PgRow,
    channels: &[Uuid],
    expected: Option<&BTreeMap<Uuid, i64>>,
) -> Result<()> {
    let variants: Value = post.get("variants");
    let mut union = BTreeSet::new();
    for channel in channels {
        let (_, ids) = effective_media(post, variant_for(&variants, &channel.to_string()))?;
        union.extend(ids);
    }
    if let Some(expected) = expected
        && (expected.len() > 1000
            || expected.len() != union.len()
            || expected.keys().any(|id| !union.contains(id))
            || expected.values().any(|version| *version <= 0))
    {
        return Err(media_version_conflict());
    }
    let ids: Vec<Uuid> = union.into_iter().collect();
    let rows = sqlx::query("SELECT id,metadata_version FROM media WHERE id=ANY($1) AND brand_id=$2 ORDER BY id FOR SHARE")
        .bind(&ids).bind(post.get::<Uuid, _>("brand_id")).fetch_all(&mut **tx).await?;
    if rows.len() != ids.len() {
        return Err(if expected.is_some() {
            media_version_conflict()
        } else {
            ApiError::conflict("Referenced media is no longer available")
        });
    }
    if let Some(expected) = expected
        && rows.iter().any(|row| {
            expected.get(&row.get::<Uuid, _>("id"))
                != Some(&i64::from(row.get::<i32, _>("metadata_version")))
        })
    {
        return Err(media_version_conflict());
    }
    Ok(())
}

fn media_version_conflict() -> ApiError {
    ApiError::conflict_code(
        "media_version_conflict",
        "Media descriptions changed or are unavailable. Refresh them before scheduling.",
    )
}

pub async fn from_post(
    tx: &mut Transaction<'_, Postgres>,
    post: &PgRow,
    channel: Uuid,
) -> Result<Value> {
    let variants: Value = post.get("variants");
    let channel_text = channel.to_string();
    let variant = variant_for(&variants, &channel_text);
    let text = variant
        .and_then(|v| v["text"].as_str())
        .unwrap_or(post.get("text"));
    let (media, ids) = effective_media(post, variant)?;
    if text.trim().is_empty() && ids.is_empty() {
        return Err(ApiError::bad("A scheduled post needs text or media"));
    }
    let options = variant
        .and_then(|v| v.get("options"))
        .cloned()
        .unwrap_or(post.get("options"));
    let metadata:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'alt_text',alt_text) FROM media WHERE id=ANY($1) AND brand_id=$2 FOR SHARE").bind(&ids).bind(post.get::<Uuid,_>("brand_id")).fetch_all(&mut **tx).await?;
    if ids.iter().collect::<HashSet<_>>().len() != metadata.len() {
        return Err(ApiError::conflict(
            "Referenced media is no longer available",
        ));
    }
    Ok(
        json!({"title":post.get::<String,_>("title"),"text":text,"media_ids":media,"options":options,"media_metadata":metadata}),
    )
}
