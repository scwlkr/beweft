use super::*;
pub(super) async fn load_publication_on(
    state: &AppState,
    db: &mut sqlx::PgConnection,
    value: &Value,
) -> Result<Publication> {
    prepare_publication_on(state, db, value, true).await
}

/// Scheduling performs the same local checks without creating a capability or
/// contacting a provider. The delivery path alone creates temporary URLs.
pub(super) async fn prepare_publication_on(
    state: &AppState,
    db: &mut sqlx::PgConnection,
    value: &Value,
    delivery: bool,
) -> Result<Publication> {
    let id = value["id"]
        .as_str()
        .ok_or_else(|| ApiError::bad("Publication identity is missing"))?;
    let payload = &value["payload"];
    let mut media = vec![];
    for raw in payload["media_ids"].as_array().unwrap_or(&vec![]) {
        let media_id: Uuid = raw
            .as_str()
            .unwrap_or("")
            .parse()
            .map_err(|_| ApiError::bad("Invalid publication media"))?;
        let row =
            sqlx::query("SELECT name,mime,storage_key FROM media WHERE id=$1 AND brand_id=$2")
                .bind(media_id)
                .bind(
                    value["brand_id"]
                        .as_str()
                        .unwrap_or("")
                        .parse::<Uuid>()
                        .map_err(|_| ApiError::bad("Missing publication brand"))?,
                )
                .fetch_one(&mut *db)
                .await?;
        let key: String = row.get("storage_key");
        if key.contains('/') || key.contains('\\') || key == "." || key == ".." {
            return Err(ApiError::bad("Invalid media storage key"));
        }
        let path = state.config.media_dir.join(key);
        let metadata = tokio::fs::metadata(&path)
            .await
            .map_err(|_| ApiError::bad("Publication media file is unavailable"))?;
        let public_url = if delivery && state.config.public_url.starts_with("https://") {
            let token = crypto::random_secret();
            sqlx::query("INSERT INTO provider_media_tokens(token_hash,media_id,expires_at) VALUES($1,$2,now()+interval '2 hours')").bind(crypto::digest(&token)).bind(media_id).execute(&mut *db).await?;
            Some(format!(
                "{}/api/v1/provider-media/{}?token={token}",
                state.config.public_url, media_id
            ))
        } else {
            None
        };
        media.push(Media {
            name: row.get("name"),
            mime: row.get("mime"),
            source: MediaSource::File {
                path,
                size: metadata.len(),
            },
            public_url,
            alt_text: payload["media_metadata"]
                .as_array()
                .and_then(|items| {
                    items
                        .iter()
                        .find(|m| m["id"].as_str() == Some(media_id.to_string().as_str()))
                })
                .and_then(|m| m["alt_text"].as_str())
                .or_else(|| payload["options"]["alt_text"].as_str())
                .unwrap_or("")
                .into(),
        });
    }
    if delivery {
        sqlx::query("DELETE FROM provider_media_tokens WHERE expires_at<now()")
            .execute(&mut *db)
            .await?;
    }
    Ok(Publication {
        key: id.into(),
        text: payload["text"].as_str().unwrap_or("").into(),
        title: payload["title"].as_str().unwrap_or("").into(),
        media,
        options: payload["options"]
            .as_object()
            .map(|m| Value::Object(m.clone()))
            .unwrap_or_else(|| json!({})),
    })
}
pub(super) async fn checkpoint(state: &AppState, id: Uuid) -> Result<Option<Value>> {
    let mut db = state.db.acquire().await?;
    checkpoint_on(state, &mut db, id).await
}
pub(super) async fn checkpoint_on(
    state: &AppState,
    db: &mut sqlx::PgConnection,
    id: Uuid,
) -> Result<Option<Value>> {
    let encrypted: Option<String> = sqlx::query_scalar(
        "SELECT encrypted_checkpoint FROM provider_delivery_checkpoints WHERE publication_id=$1",
    )
    .bind(id)
    .fetch_optional(&mut *db)
    .await?;
    encrypted
        .map(|s| {
            let plain = crypto::decrypt(encryption_key(state)?, id, &s)
                .map_err(|_| ApiError::bad("Provider checkpoint could not be decrypted"))?;
            serde_json::from_str(&plain).map_err(|_| ApiError::bad("Invalid provider checkpoint"))
        })
        .transpose()
}
