use crate::{
    auth::Auth,
    crypto,
    error::{ApiError, Result},
};
use axum::Json;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    version: u8,
    context: String,
    created_at: DateTime<Utc>,
    id: Uuid,
}

pub struct Page {
    pub limit: i64,
    pub created_at: Option<DateTime<Utc>>,
    pub id: Option<Uuid>,
    context: String,
}
impl Page {
    pub fn new(
        collection: &str,
        limit: Option<u32>,
        cursor: Option<&str>,
        filters: Value,
        auth: &Auth,
    ) -> Result<Option<Self>> {
        let Some(limit) = limit else {
            if cursor.is_some() {
                return Err(ApiError::bad("A cursor requires limit"));
            }
            return Ok(None);
        };
        if !(1..=10).contains(&limit) {
            return Err(ApiError::bad("limit must be between 1 and 10"));
        }
        let mut brands = auth.brand_ids.clone();
        brands.sort_unstable();
        brands.dedup();
        let context = crypto::digest(&json!({"collection":collection,"order":"created_at_id_desc_v1","filters":filters,"brands":brands}).to_string());
        let decoded = match cursor {
            None => None,
            Some(raw) => {
                if raw.len() > 2048 {
                    return Err(ApiError::bad("Invalid or incompatible cursor"));
                }
                let bytes = URL_SAFE_NO_PAD
                    .decode(raw)
                    .map_err(|_| ApiError::bad("Invalid or incompatible cursor"))?;
                let value: Cursor = serde_json::from_slice(&bytes)
                    .map_err(|_| ApiError::bad("Invalid or incompatible cursor"))?;
                if value.version != 1 || value.context != context {
                    return Err(ApiError::bad("Invalid or incompatible cursor"));
                }
                Some(value)
            }
        };
        Ok(Some(Self {
            limit: i64::from(limit),
            created_at: decoded.as_ref().map(|c| c.created_at),
            id: decoded.map(|c| c.id),
            context,
        }))
    }
    pub fn finish(&self, mut rows: Vec<(Value, DateTime<Utc>, Uuid)>) -> Result<Json<Value>> {
        let has_more = rows.len() > self.limit as usize;
        rows.truncate(self.limit as usize);
        let next_cursor = if has_more {
            let (_, created_at, id) = rows
                .last()
                .ok_or_else(|| ApiError::internal("empty page"))?;
            let cursor = Cursor {
                version: 1,
                context: self.context.clone(),
                created_at: *created_at,
                id: *id,
            };
            Some(URL_SAFE_NO_PAD.encode(serde_json::to_vec(&cursor).map_err(ApiError::internal)?))
        } else {
            None
        };
        Ok(Json(
            json!({"data":rows.into_iter().map(|(value, _, _)| value).collect::<Vec<_>>(), "next_cursor":next_cursor}),
        ))
    }
}
