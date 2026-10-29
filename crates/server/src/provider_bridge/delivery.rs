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

/// Check the attempt and the database clock in the same statement that writes
/// progress. A suspended worker may resume after the lease even if its network
/// request began while the attempt was valid.
pub(super) async fn save_attempt_checkpoint_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    state: &AppState,
    id: Uuid,
    attempt: Uuid,
    cp: &Value,
) -> Result<()> {
    let encrypted = crypto::encrypt(
        encryption_key(state)?,
        id,
        &serde_json::to_string(cp).map_err(ApiError::internal)?,
    )
    .map_err(ApiError::internal)?;
    let changed = sqlx::query("INSERT INTO provider_delivery_checkpoints(publication_id,encrypted_checkpoint) SELECT p.id,$3 FROM publications p JOIN publication_attempts a ON a.publication_id=p.id WHERE p.id=$1 AND p.state='sending' AND p.lease_until>clock_timestamp() AND a.id=$2 AND a.state='sending' ON CONFLICT(publication_id) DO UPDATE SET encrypted_checkpoint=EXCLUDED.encrypted_checkpoint,updated_at=clock_timestamp()")
        .bind(id)
        .bind(attempt)
        .bind(encrypted)
        .execute(&mut **tx)
        .await?;
    if changed.rows_affected() != 1 {
        return Err(ApiError::conflict(
            "Delivery attempt expired or was replaced before saving provider progress",
        ));
    }
    Ok(())
}
