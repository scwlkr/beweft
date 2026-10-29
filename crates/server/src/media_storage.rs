use crate::{
    error::{ApiError, Result},
    state::AppState,
};
use chrono::{Duration, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use std::path::PathBuf;
use uuid::Uuid;
const QUOTA_LOCK: i64 = 9711302;
pub async fn lock(tx: &mut Transaction<'_, Postgres>) -> Result<()> {
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(QUOTA_LOCK)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
/// The guard also handles aborted request futures. A committed media row always wins over cleanup.
pub struct UploadGuard {
    id: Uuid,
    db: PgPool,
    root: PathBuf,
    armed: bool,
}
impl UploadGuard {
    pub fn finish(&mut self) {
        self.armed = false;
    }
}
impl Drop for UploadGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let id = self.id;
        let db = self.db.clone();
        let root = self.root.clone();
        tokio::spawn(async move {
            let Ok(mut tx) = db.begin().await else {
                return;
            };
            if lock(&mut tx).await.is_err() {
                return;
            }
            let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM media WHERE id=$1)")
                .bind(id)
                .fetch_one(&mut *tx)
                .await
                .unwrap_or(true);
            if !exists {
                let _ = tokio::fs::remove_file(root.join(format!("{id}.bin"))).await;
            }
            let _ = tokio::fs::remove_file(root.join(format!(".upload-{id}.part"))).await;
            let _ = sqlx::query("DELETE FROM media_upload_reservations WHERE id=$1")
                .bind(id)
                .execute(&mut *tx)
                .await;
            let _ = tx.commit().await;
        });
    }
}
pub async fn reserve(state: &AppState, id: Uuid, bytes: u64) -> Result<UploadGuard> {
    if bytes == 0 || bytes > state.config.max_upload_bytes {
        return Err(ApiError::bad("Invalid upload reservation"));
    }
    let mut tx = state.db.begin().await?;
    lock(&mut tx).await?;
    let expired: Vec<Uuid> = sqlx::query_scalar(
        "DELETE FROM media_upload_reservations WHERE expires_at<now() RETURNING id",
    )
    .fetch_all(&mut *tx)
    .await?;
    for old in expired {
        let _ = tokio::fs::remove_file(state.config.media_dir.join(format!(".upload-{old}.part")))
            .await;
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM media WHERE id=$1)")
            .bind(old)
            .fetch_one(&mut *tx)
            .await?;
        if !exists {
            let _ = tokio::fs::remove_file(state.config.media_dir.join(format!("{old}.bin"))).await;
        }
    }
    let used:i64=sqlx::query_scalar("SELECT COALESCE((SELECT sum(size) FROM media),0)::bigint + COALESCE((SELECT sum(reserved_bytes) FROM media_upload_reservations),0)::bigint").fetch_one(&mut *tx).await?;
    if (used as u64).saturating_add(bytes) > state.config.media_quota_bytes {
        return Err(ApiError::bad(
            "Media storage quota reached. Remove unused media or increase BEWEFT_MEDIA_QUOTA_BYTES.",
        ));
    }
    sqlx::query(
        "INSERT INTO media_upload_reservations(id,reserved_bytes,expires_at) VALUES($1,$2,$3)",
    )
    .bind(id)
    .bind(bytes as i64)
    .bind(Utc::now() + Duration::hours(1))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(UploadGuard {
        id,
        db: state.db.clone(),
        root: state.config.media_dir.clone(),
        armed: true,
    })
}
