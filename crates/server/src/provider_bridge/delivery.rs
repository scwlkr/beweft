use super::*;
pub(crate) async fn save_checkpoint_tx(
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
    stage_started: chrono::DateTime<chrono::Utc>,
    wait_seconds: Option<u64>,
) -> Result<Value> {
    let mut cp = cp.clone();
    let managed_checkpoint = cp["stage"].as_str().is_some_and(|stage| {
        stage.starts_with("x_") || stage.starts_with("meta_") || stage.starts_with("fb_reel_")
    });
    if managed_checkpoint {
        cp["attempt_id"] = json!(attempt);
        cp["checkpoint_id"] = json!(Uuid::new_v4());
        if let Some(object) = cp.as_object_mut() {
            object.remove("not_published_attestation");
        }
    }
    let expiry = if managed_checkpoint {
        cp["expires_after_secs"]
            .as_i64()
            .filter(|seconds| *seconds > 0)
    } else {
        None
    };
    let encrypted = crypto::encrypt(
        encryption_key(state)?,
        id,
        &serde_json::to_string(&cp).map_err(ApiError::internal)?,
    )
    .map_err(ApiError::internal)?;
    let checkpoint_id = cp["checkpoint_id"]
        .as_str()
        .and_then(|id| id.parse::<Uuid>().ok());
    let changed = sqlx::query("INSERT INTO provider_delivery_checkpoints(publication_id,encrypted_checkpoint,not_before,expires_at,checkpoint_id,attempt_id) SELECT p.id,$3,CASE WHEN $5::bigint IS NULL THEN NULL ELSE clock_timestamp()+($5::bigint * interval '1 second') END,CASE WHEN $6::bigint IS NULL THEN NULL ELSE $4::timestamptz+($6::bigint * interval '1 second') END,COALESCE($7,gen_random_uuid()),$2 FROM publications p JOIN publication_attempts a ON a.publication_id=p.id WHERE p.id=$1 AND p.state='sending' AND p.lease_until>clock_timestamp() AND a.id=$2 AND a.state='sending' ON CONFLICT(publication_id) DO UPDATE SET encrypted_checkpoint=EXCLUDED.encrypted_checkpoint,not_before=EXCLUDED.not_before,expires_at=LEAST(provider_delivery_checkpoints.expires_at,EXCLUDED.expires_at),checkpoint_id=EXCLUDED.checkpoint_id,attempt_id=EXCLUDED.attempt_id,updated_at=clock_timestamp()")
        .bind(id)
        .bind(attempt)
        .bind(encrypted)
        .bind(stage_started)
        .bind(wait_seconds.map(|seconds| seconds as i64))
        .bind(expiry)
        .bind(checkpoint_id)
        .execute(&mut **tx)
        .await?;
    if changed.rows_affected() != 1 {
        return Err(ApiError::conflict(
            "Delivery attempt expired or was replaced before saving provider progress",
        ));
    }
    Ok(cp)
}
