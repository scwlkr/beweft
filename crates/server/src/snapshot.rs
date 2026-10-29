use crate::error::{ApiError, Result};
use serde_json::{Value, json};
use sqlx::{Postgres, Row, Transaction, postgres::PgRow};
use uuid::Uuid;
pub async fn from_post(
    tx: &mut Transaction<'_, Postgres>,
    post: &PgRow,
    channel: Uuid,
) -> Result<Value> {
    let variants: Value = post.get("variants");
    let channel_text = channel.to_string();
    let variant = variants.as_array().and_then(|v| {
        v.iter()
            .find(|v| v["channel_id"].as_str() == Some(&channel_text))
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
    let ids: Vec<Uuid> = serde_json::from_value(media.clone()).map_err(ApiError::internal)?;
    let metadata:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'alt_text',alt_text) FROM media WHERE id=ANY($1) AND brand_id=$2 FOR SHARE").bind(&ids).bind(post.get::<Uuid,_>("brand_id")).fetch_all(&mut **tx).await?;
    if ids.iter().collect::<std::collections::HashSet<_>>().len() != metadata.len() {
        return Err(ApiError::conflict(
            "Referenced media is no longer available",
        ));
    }
    Ok(
        json!({"title":post.get::<String,_>("title"),"text":text,"media_ids":media,"options":options,"media_metadata":metadata}),
    )
}
