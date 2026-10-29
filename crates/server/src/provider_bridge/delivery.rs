use super::*;
pub(super) async fn save_checkpoint_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    state: &AppState,
    id: Uuid,
    cp: &Value,
) -> Result<()> {
    let encrypted = crypto::encrypt(
        encryption_key(state)?,
        id,
        &serde_json::to_string(cp).map_err(ApiError::internal)?,
    )
    .map_err(ApiError::internal)?;
    sqlx::query("INSERT INTO provider_delivery_checkpoints(publication_id,encrypted_checkpoint) VALUES($1,$2) ON CONFLICT(publication_id) DO UPDATE SET encrypted_checkpoint=EXCLUDED.encrypted_checkpoint,updated_at=now()").bind(id).bind(encrypted).execute(&mut **tx).await?;
    Ok(())
}
