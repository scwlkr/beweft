use crate::{
    auth::Auth,
    error::{ApiError, Result, data, required, uuid},
    listing::Page,
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, Query, State},
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use uuid::Uuid;
const SELECT: &str = "SELECT (to_jsonb(p)-'lease_until')||jsonb_build_object('channel_name',c.name,'provider',c.provider) FROM publications p JOIN channels c ON c.id=p.channel_id";
const SUMMARY: &str = "jsonb_build_object('id',p.id,'post_id',p.post_id,'brand_id',p.brand_id,'channel_id',p.channel_id,'channel_name',left(c.name,100),'provider',left(c.provider,64),'post_version',p.post_version,'scheduled_at',p.scheduled_at,'scheduling_mode',p.scheduling_mode,'state',p.state,'created_at',p.created_at,'updated_at',p.updated_at)";
#[derive(Deserialize, Default)]
pub struct PublicationListFilter {
    brand_id: Option<Uuid>,
    channel_id: Option<Uuid>,
    id: Option<Uuid>,
    group: Option<String>,
    view: Option<String>,
    limit: Option<u32>,
    cursor: Option<String>,
}
pub async fn get_publication(db: &PgPool, id: Uuid) -> Result<Value> {
    Ok(sqlx::query_scalar(&format!("{SELECT} WHERE p.id=$1"))
        .bind(id)
        .fetch_one(db)
        .await?)
}
pub(crate) async fn get_publication_summaries(db: &PgPool, ids: &[Uuid]) -> Result<Vec<Value>> {
    let query = format!(
        "SELECT {SUMMARY} FROM unnest($1::uuid[]) WITH ORDINALITY AS wanted(id,position) JOIN publications p ON p.id=wanted.id JOIN channels c ON c.id=p.channel_id ORDER BY wanted.position"
    );
    Ok(sqlx::query_scalar(&query).bind(ids).fetch_all(db).await?)
}
pub async fn list(
    State(state): State<AppState>,
    auth: Auth,
    Query(filter): Query<PublicationListFilter>,
) -> Result<Json<Value>> {
    if let Some(id) = filter.brand_id {
        auth.brand(id)?;
    }
    if let Some(id) = filter.channel_id {
        auth.brand(crate::catalog::channel_brand(&state, id).await?)?;
    }
    if filter
        .group
        .as_deref()
        .is_some_and(|g| !matches!(g, "queue" | "history"))
        || filter.view.as_deref().is_some_and(|v| v != "summary")
    {
        return Err(ApiError::bad("Unsupported publication group or view"));
    }
    let value = if filter.view.as_deref() == Some("summary") {
        SUMMARY
    } else {
        "(to_jsonb(p)-'lease_until')||jsonb_build_object('channel_name',c.name,'provider',c.provider)"
    };
    let filters = json!({"brand_id":filter.brand_id,"channel_id":filter.channel_id,"id":filter.id,"group":filter.group,"view":filter.view});
    let page = Page::new(
        "publications",
        filter.limit,
        filter.cursor.as_deref(),
        filters,
        &auth,
    )?;
    let condition = "($1::uuid IS NULL OR p.brand_id=$1) AND ($2::uuid IS NULL OR p.channel_id=$2) AND (cardinality($3::uuid[])=0 OR p.brand_id=ANY($3)) AND ($4::uuid IS NULL OR p.id=$4) AND ($5::text IS NULL OR ($5='queue' AND p.state IN ('queued','sending')) OR ($5='history' AND p.state NOT IN ('queued','sending')))";
    if let Some(page) = page {
        let query = format!(
            "SELECT {value},p.created_at,p.id FROM publications p JOIN channels c ON c.id=p.channel_id WHERE {condition} AND ($6::timestamptz IS NULL OR (p.created_at,p.id)<($6,$7)) ORDER BY p.created_at DESC,p.id DESC LIMIT $8"
        );
        let rows = sqlx::query_as(&query)
            .bind(filter.brand_id)
            .bind(filter.channel_id)
            .bind(&auth.brand_ids)
            .bind(filter.id)
            .bind(filter.group.as_deref())
            .bind(page.created_at)
            .bind(page.id)
            .bind(page.limit + 1)
            .fetch_all(&state.db)
            .await?;
        page.finish(rows)
    } else {
        let query = format!(
            "SELECT {value} FROM publications p JOIN channels c ON c.id=p.channel_id WHERE {condition} ORDER BY p.scheduled_at,p.created_at"
        );
        let rows: Vec<Value> = sqlx::query_scalar(&query)
            .bind(filter.brand_id)
            .bind(filter.channel_id)
            .bind(&auth.brand_ids)
            .bind(filter.id)
            .bind(filter.group.as_deref())
            .fetch_all(&state.db)
            .await?;
        Ok(data(rows))
    }
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
    Json(input): Json<crate::contracts::ManualCompleteRequest>,
) -> Result<Json<Value>> {
    let body = serde_json::to_value(input).map_err(ApiError::internal)?;
    change(&state, &auth, id, "manual_complete", body).await
}
pub async fn reconcile(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(input): Json<crate::contracts::ReconcileRequest>,
) -> Result<Json<Value>> {
    let body = serde_json::to_value(input).map_err(ApiError::internal)?;
    change(&state, &auth, id, "reconcile", body).await
}
pub async fn reschedule(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(input): Json<crate::contracts::RescheduleRequest>,
) -> Result<Json<Value>> {
    let body = serde_json::to_value(input).map_err(ApiError::internal)?;
    change(&state, &auth, id, "reschedule", body).await
}
async fn change(
    state: &AppState,
    auth: &Auth,
    id: Uuid,
    action: &str,
    mut body: Value,
) -> Result<Json<Value>> {
    auth.scope("publishing:write")?;
    let mut tx = state.db.begin().await?;
    let channel: Uuid = sqlx::query_scalar("SELECT channel_id FROM publications WHERE id=$1")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    crate::schedules::lock_channel(&mut tx, channel).await?;
    let row=sqlx::query("SELECT p.*,c.provider,c.connection_identity FROM publications p JOIN channels c ON c.id=p.channel_id WHERE p.id=$1 FOR UPDATE OF p").bind(id).fetch_one(&mut *tx).await?;
    auth.brand(row.get("brand_id"))?;
    if ["retry", "reschedule"].contains(&action) {
        crate::brand_admin::active(&mut tx, row.get("brand_id")).await?;
    }
    let current: String = row.get("state");
    let provider: String = row.get("provider");
    let payload: Value = row.get("payload");
    let x_thread = provider == "x" && crate::provider_bridge::has_x_thread(&payload);
    if body.get("thread_ids").is_some()
        && (!x_thread || action != "reconcile" || body["resolution"] != "published")
    {
        return Err(ApiError::bad(
            "Thread IDs are only valid for X thread completion",
        ));
    }
    if body.get("thread_root_definitively_ended").is_some()
        && (!x_thread || action != "reconcile" || body["resolution"] != "not_published")
    {
        return Err(ApiError::bad(
            "The root-ended attestation is only valid for an X thread not-published resolution",
        ));
    }
    let mut x_checkpoint = if provider == "x"
        && (["retry", "reconcile"].contains(&action)
            || (x_thread && ["cancel", "reschedule"].contains(&action)))
    {
        crate::provider_bridge::x_checkpoint_on(state, &mut tx, id).await?
    } else {
        None
    };
    if x_thread && ["retry", "cancel", "reschedule"].contains(&action) {
        if let Some(cp) = x_checkpoint.as_ref() {
            crate::provider_bridge::x_thread_status(&payload, cp)?;
            if !crate::provider_bridge::x_thread_pre_public(cp) {
                return Err(ApiError::conflict(
                    "An X thread may already be public. Inspect its reply chain before recovery; the whole thread cannot be restarted or canceled.",
                ));
            }
        } else {
            // A new queued thread has no attempt. After any attempt, a missing
            // checkpoint cannot prove no write occurred; a recorded safe reset
            // is the sole exception, fenced against later attempts.
            if !crate::provider_bridge::valid_x_thread_snapshot(&payload) {
                return Err(ApiError::conflict(
                    "The saved X thread is invalid; recovery cannot proceed",
                ));
            }
            let no_attempts: bool = sqlx::query_scalar(
                "SELECT NOT EXISTS(SELECT 1 FROM publication_attempts WHERE publication_id=$1)",
            )
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
            let safe = no_attempts
                || crate::provider_bridge::x_thread_reset_covers_attempts(&mut tx, id).await?;
            if !safe {
                return Err(ApiError::conflict(
                    "X thread progress is missing. Inspect its actual reply chain; whole-thread retry or cancellation is unavailable.",
                ));
            }
        }
    }
    if x_thread && action == "reconcile" {
        let cp = x_checkpoint.as_mut().ok_or_else(|| {
            ApiError::conflict(
                "The X thread checkpoint is missing; its outcome cannot be safely resolved",
            )
        })?;
        if body["resolution"] == "published" {
            auth.owner_session()?;
            let (root, url) = crate::provider_bridge::x_thread_completion(&payload, cp, &body)?;
            body["external_id"] = json!(root);
            body["external_url"] = json!(url);
        } else {
            let status = crate::provider_bridge::x_thread_status(&payload, cp)?;
            if crate::provider_bridge::x_thread_pre_public(cp) {
                if body.get("thread_root_definitively_ended").is_some() {
                    return Err(ApiError::bad(
                        "A root-ended attestation is only valid for a started X thread root",
                    ));
                }
            } else {
                auth.owner_session()?;
                if !crate::provider_bridge::x_thread_current_root_started(cp)
                    || status["phase"] != "request_started"
                    || status["next_index"] != 0
                    || status["confirmed_ids"]
                        .as_array()
                        .is_none_or(|ids| !ids.is_empty())
                    || body["observed_attempt_id"] != status["attempt_id"]
                    || body["observed_checkpoint_id"] != status["checkpoint_id"]
                    || body["sender_stopped"] != true
                    || body["provider_inspected"] != true
                    || body["thread_root_definitively_ended"] != true
                {
                    return Err(ApiError::conflict(
                        "Only an unconfirmed root can be recorded not published after the owner confirms the remote operation definitively ended, using the current IDs.",
                    ));
                }
                cp["thread_root_ended_attestation"] = json!({"attempt_id":status["attempt_id"],"checkpoint_id":status["checkpoint_id"],"thread_root_definitively_ended":true});
            }
        }
    }
    let mut meta_checkpoint = if matches!(provider.as_str(), "facebook" | "instagram" | "threads")
        && ["retry", "reconcile"].contains(&action)
    {
        crate::provider_bridge::meta_checkpoint_on(state, &mut tx, id).await?
    } else {
        None
    };
    let mut linkedin_checkpoint =
        if provider == "linkedin" && ["retry", "reconcile"].contains(&action) {
            crate::provider_bridge::checkpoint_for_recovery(state, &mut tx, id).await?
        } else {
            None
        };
    let mut google_checkpoint =
        if provider == "google_business" && ["retry", "reconcile"].contains(&action) {
            crate::provider_bridge::checkpoint_for_recovery(state, &mut tx, id).await?
        } else {
            None
        };
    if let Some(cp) = meta_checkpoint.as_ref() {
        let (attempt, checkpoint) = crate::provider_bridge::meta_identity(cp)?;
        let matches_row: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM provider_delivery_checkpoints cp JOIN publication_attempts a ON a.id=cp.attempt_id AND a.publication_id=cp.publication_id WHERE cp.publication_id=$1 AND cp.attempt_id=$2 AND cp.checkpoint_id=$3)")
            .bind(id).bind(attempt).bind(checkpoint).fetch_one(&mut *tx).await?;
        if !matches_row {
            return Err(ApiError::conflict(
                "Meta delivery identity changed; reload before recovery",
            ));
        }
    }
    if let Some(cp) = google_checkpoint.as_ref() {
        let (attempt, checkpoint) = crate::provider_bridge::google_identity(cp)?;
        let matches_row: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM provider_delivery_checkpoints cp JOIN publication_attempts a ON a.id=cp.attempt_id AND a.publication_id=cp.publication_id WHERE cp.publication_id=$1 AND cp.attempt_id=$2 AND cp.checkpoint_id=$3)")
            .bind(id).bind(attempt).bind(checkpoint).fetch_one(&mut *tx).await?;
        if !matches_row {
            return Err(ApiError::conflict(
                "Google delivery identity changed; reload before recovery",
            ));
        }
    }
    if provider == "google_business" && action == "reconcile" {
        auth.owner_session()?;
        let cp = google_checkpoint.as_ref().ok_or_else(|| ApiError::conflict("The Google delivery has no recovery identity; inspect the original attempt before resolving it"))?;
        let (attempt, checkpoint) = crate::provider_bridge::google_identity(cp)?;
        if body["observed_attempt_id"] != json!(attempt)
            || body["observed_checkpoint_id"] != json!(checkpoint)
            || body["sender_stopped"] != true
            || body["provider_inspected"] != true
        {
            return Err(ApiError::conflict(
                "Stop the original sender and inspect Google using the current attempt and checkpoint IDs. For not-published, inspection must establish that the remote operation definitively failed, stopped, or was canceled; a missing post or stopped local sender alone is not proof.",
            ));
        }
    }
    if action == "retry"
        && let Some(cp) = google_checkpoint.as_ref()
    {
        let (attempt, checkpoint) = crate::provider_bridge::google_identity(cp)?;
        if cp["stage"] != "google_post_rejected"
            && !crate::provider_bridge::google_attested(cp, attempt, checkpoint)
        {
            return Err(ApiError::conflict(
                "Google may still publish the original post. Reconcile the exact delivery before retrying.",
            ));
        }
        if current == "failed" {
            sqlx::query("INSERT INTO audit_events(id,owner_id,action,subject_id,detail) VALUES($1,$2,'retry_google_delivery',$3,$4)")
                .bind(Uuid::new_v4()).bind(auth.owner_id).bind(id)
                .bind(json!({"attempt_id":attempt,"checkpoint_id":checkpoint,"stage":cp["stage"],"post_name":cp["name"],"owner_attestation":cp["not_published_attestation"]}))
                .execute(&mut *tx).await?;
            sqlx::query("DELETE FROM provider_delivery_checkpoints WHERE publication_id=$1")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
    }
    if action == "retry"
        && let Some(cp) = linkedin_checkpoint.as_ref()
    {
        let (attempt, checkpoint) = crate::provider_bridge::linkedin_identity(cp)?;
        if !crate::provider_bridge::linkedin_attested(cp, attempt, checkpoint) {
            return Err(ApiError::conflict(
                "Stop the sender, inspect LinkedIn, and record not-published for the current attempt and checkpoint before retrying",
            ));
        }
        if current == "failed" {
            sqlx::query("INSERT INTO audit_events(id,owner_id,action,subject_id,detail) VALUES($1,$2,'retry_linkedin_delivery',$3,$4)")
                .bind(Uuid::new_v4()).bind(auth.owner_id).bind(id)
                .bind(json!({"provider":provider,"attempt_id":attempt,"checkpoint_id":checkpoint,"stage":cp["stage"],"upload_id":cp["upload_id"],"media_ids":cp["media_ids"],"owner_attestation":cp["not_published_attestation"]}))
                .execute(&mut *tx).await?;
            sqlx::query("DELETE FROM provider_delivery_checkpoints WHERE publication_id=$1")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
    }
    if action == "retry"
        && let Some(cp) = meta_checkpoint.as_ref()
    {
        let (attempt, checkpoint) = crate::provider_bridge::meta_identity(cp)?;
        if crate::provider_bridge::meta_public_boundary(cp)
            && !crate::provider_bridge::meta_attested(cp, attempt, checkpoint)
        {
            return Err(ApiError::conflict(
                "Stop the sender, inspect the provider, and record not-published for the current Meta attempt and checkpoint before retrying",
            ));
        }
        if current == "failed" {
            // This is an explicit retry, never a timeout-driven replacement. Keep
            // provider identifiers and the attested checkpoint in the audit trail.
            sqlx::query("INSERT INTO audit_events(id,owner_id,action,subject_id,detail) VALUES($1,$2,'retry_meta_delivery',$3,$4)")
                .bind(Uuid::new_v4()).bind(auth.owner_id).bind(id)
                .bind(json!({"provider":provider,"attempt_id":attempt,"checkpoint_id":checkpoint,"stage":cp["stage"],"container_id":cp["container_id"],"children":cp["children"],"video_id":cp["video_id"],"owner_attestation":cp["not_published_attestation"]}))
                .execute(&mut *tx).await?;
            sqlx::query("DELETE FROM provider_delivery_checkpoints WHERE publication_id=$1")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
    }
    if action == "retry"
        && x_checkpoint.as_ref().is_some_and(|cp| {
            cp["stage"]
                .as_str()
                .is_some_and(|stage| stage.starts_with("x_"))
        })
    {
        return Err(ApiError::conflict(
            "Inspect provider status and reset the X checkpoint before retrying",
        ));
    }
    if action == "retry" && current == "failed" && provider == "bluesky" {
        crate::provider_bridge::prepare_bluesky_video_retry_tx(state, &mut tx, id).await?;
    }
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
        "reconcile"
            if current == "uncertain"
                || (current == "failed"
                    && ((x_thread && body["resolution"] == "published")
                        || (body["resolution"] == "not_published"
                            && (x_checkpoint
                                .as_ref()
                                .is_some_and(crate::provider_bridge::x_public_boundary)
                                || meta_checkpoint.is_some()
                                || linkedin_checkpoint.is_some()
                                || google_checkpoint.is_some())))) =>
        {
            required(&body, "note")?;
            match body["resolution"].as_str() {
                Some("published") => "published",
                Some("not_published") => {
                    if let Some(cp) = google_checkpoint.as_mut() {
                        let (attempt, checkpoint) = crate::provider_bridge::google_identity(cp)?;
                        cp["not_published_attestation"] = json!({"attempt_id":attempt,"checkpoint_id":checkpoint,"remote_definitively_ended":true});
                        crate::provider_bridge::save_recovery_checkpoint_tx(&mut tx, state, id, cp)
                            .await?;
                    }
                    if let Some(cp) = linkedin_checkpoint.as_mut() {
                        auth.owner_session()?;
                        let (attempt, checkpoint) = crate::provider_bridge::linkedin_identity(cp)?;
                        if body["observed_attempt_id"] != json!(attempt)
                            || body["observed_checkpoint_id"] != json!(checkpoint)
                            || body["sender_stopped"] != true
                            || body["provider_inspected"] != true
                        {
                            return Err(ApiError::conflict(
                                "Stop the original sender, inspect LinkedIn, and attest not-published using the current attempt and checkpoint IDs",
                            ));
                        }
                        cp["not_published_attestation"] =
                            json!({"attempt_id":attempt,"checkpoint_id":checkpoint});
                        crate::provider_bridge::save_recovery_checkpoint_tx(&mut tx, state, id, cp)
                            .await?;
                    }
                    if let Some(cp) = meta_checkpoint.as_mut() {
                        auth.owner_session()?;
                        let (attempt, checkpoint) = crate::provider_bridge::meta_identity(cp)?;
                        if body["observed_attempt_id"] != json!(attempt)
                            || body["observed_checkpoint_id"] != json!(checkpoint)
                            || body["sender_stopped"] != true
                            || body["provider_inspected"] != true
                        {
                            return Err(ApiError::conflict(
                                "Stop the original sender, inspect the provider, and attest not-published using the current Meta attempt and checkpoint IDs",
                            ));
                        }
                        let public = crate::provider_bridge::meta_public_boundary(cp);
                        if public && body["remote_definitively_ended"] != true {
                            return Err(ApiError::conflict(
                                "Confirm the remote Meta operation definitively failed, stopped, or was canceled. An absent post or stopped local sender alone is not proof.",
                            ));
                        }
                        cp["not_published_attestation"] = json!({
                            "attempt_id":attempt,"checkpoint_id":checkpoint,
                            "remote_definitively_ended": if public { json!(true) } else { Value::Null }
                        });
                        crate::provider_bridge::save_recovery_checkpoint_tx(&mut tx, state, id, cp)
                            .await?;
                    }
                    if let Some(cp) = x_checkpoint
                        .as_mut()
                        .filter(|cp| crate::provider_bridge::x_public_boundary(cp))
                    {
                        auth.owner_session()?;
                        let (attempt, checkpoint) = crate::provider_bridge::x_identity(cp)?;
                        if body["observed_attempt_id"] != json!(attempt)
                            || body["observed_checkpoint_id"] != json!(checkpoint)
                            || body["sender_stopped"] != true
                            || body["provider_inspected"] != true
                        {
                            return Err(ApiError::conflict(
                                "Stop the original sender, inspect X, and attest not-published using the current attempt and checkpoint IDs",
                            ));
                        }
                        cp["not_published_attestation"] =
                            json!({"attempt_id":attempt,"checkpoint_id":checkpoint});
                        crate::provider_bridge::save_recovery_checkpoint_tx(&mut tx, state, id, cp)
                            .await?;
                    }
                    "failed"
                }
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
    let mut external_id = body["external_id"].as_str();
    if provider == "google_business"
        && action == "reconcile"
        && target == "published"
        && let Some(supplied) = external_id
    {
        let location: Option<String> = row.get("connection_identity");
        let location = location.ok_or_else(|| ApiError::conflict("Google channel location is unavailable; omit the external ID and inspect the original delivery"))?;
        beweft_providers::google_post_name_for_location(supplied, &location).map_err(|_| {
            ApiError::bad(
                "Google post ID must be a canonical resource name for this channel's location",
            )
        })?;
    }
    if provider == "google_business"
        && action == "reconcile"
        && target == "published"
        && let Some(saved) = google_checkpoint
            .as_ref()
            .and_then(|cp| cp["name"].as_str())
    {
        if external_id.is_some_and(|supplied| supplied != saved) {
            return Err(ApiError::conflict(
                "The supplied Google post ID differs from the saved post resource",
            ));
        }
        external_id = Some(saved);
    }
    sqlx::query("UPDATE publications SET state=$2,scheduled_at=$3,external_id=COALESCE($4,external_id),external_url=COALESCE($5,external_url),last_error=CASE WHEN $2 IN ('queued','published','canceled') THEN NULL ELSE last_error END,lease_until=NULL,scheduling_mode=CASE WHEN $6 THEN 'exact' ELSE scheduling_mode END,updated_at=now() WHERE id=$1").bind(id).bind(target).bind(scheduled_at).bind(external_id).bind(external_url).bind(action == "reschedule").execute(&mut *tx).await?;
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
    Json(input): Json<crate::contracts::ReorderQueueRequest>,
) -> Result<Json<Value>> {
    let body = serde_json::to_value(input).map_err(ApiError::internal)?;
    auth.scope("publishing:write")?;
    let channel = uuid(&body, "channel_id")?;
    auth.brand(crate::catalog::channel_brand(&state, channel).await?)?;
    let ids: Vec<Uuid> = serde_json::from_value(body["publication_ids"].clone())
        .map_err(|_| ApiError::bad("publication_ids must be UUIDs"))?;
    let mut tx = state.db.begin().await?;
    crate::schedules::lock_channel(&mut tx, channel).await?;
    let rows=sqlx::query("SELECT id,scheduled_at FROM publications WHERE channel_id=$1 AND state='queued' AND scheduling_mode='queue' ORDER BY scheduled_at,id FOR UPDATE").bind(channel).fetch_all(&mut *tx).await?;
    let actual: std::collections::HashSet<Uuid> = rows.iter().map(|r| r.get("id")).collect();
    let requested: std::collections::HashSet<Uuid> = ids.iter().copied().collect();
    if actual != requested || ids.len() != requested.len() {
        return Err(ApiError::conflict(
            "Include every queue-slot publication exactly once; exact-date publications cannot be reordered",
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
