use crate::{
    auth::Auth,
    contracts::{PrioritizeRequest, ReplacePublicationRequest},
    error::{ApiError, Result, data},
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, State},
};
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;
/// Read the exact audit-linked successor in the same database snapshot as the
/// original. No link is inconclusive: an earlier replacement may still run.
pub async fn replacement_status(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let row = sqlx::query(
        r#"WITH visible AS (
            SELECT p.id,p.brand_id,p.post_id,p.channel_id,
              jsonb_build_object('id',p.id,'post_id',p.post_id,'brand_id',p.brand_id,
                'channel_id',p.channel_id,'channel_name',left(c.name,100),
                'provider',left(c.provider,64),'post_version',p.post_version,
                'scheduled_at',p.scheduled_at,'scheduling_mode',p.scheduling_mode,
                'state',p.state,'created_at',p.created_at,'updated_at',p.updated_at) AS summary
            FROM publications p JOIN channels c ON c.id=p.channel_id
            WHERE p.id=$1 AND (cardinality($2::uuid[])=0 OR p.brand_id=ANY($2))
        )
        SELECT v.summary AS original,a.events,a.owner_matches,(
            SELECT jsonb_build_object('id',r.id,'post_id',r.post_id,'brand_id',r.brand_id,
                'channel_id',r.channel_id,'channel_name',left(rc.name,100),
                'provider',left(rc.provider,64),'post_version',r.post_version,
                'scheduled_at',r.scheduled_at,'scheduling_mode',r.scheduling_mode,
                'state',r.state,'created_at',r.created_at,'updated_at',r.updated_at)
            FROM publications r JOIN channels rc ON rc.id=r.channel_id
            WHERE r.id::text=a.replacement_id AND r.brand_id=v.brand_id
                AND r.post_id=v.post_id AND r.channel_id=v.channel_id
        ) AS replacement
        FROM visible v CROSS JOIN LATERAL (
            SELECT count(*) AS events,COALESCE(bool_and(owner_id=$3),false) AS owner_matches,
                max(detail->>'replacement_id') AS replacement_id
            FROM audit_events WHERE action='replace_queued_content' AND subject_id=v.id
        ) a"#,
    )
    .bind(id)
    .bind(&auth.brand_ids)
    .bind(auth.owner_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(ApiError::missing)?;
    let original: Value = row.get("original");
    let replacement: Option<Value> = row.get("replacement");
    let events: i64 = row.get("events");
    let owner_matches: bool = row.get("owner_matches");
    if events > 1
        || (events == 1
            && (!owner_matches || replacement.is_none() || original["state"] != "canceled"))
    {
        return Err(ApiError::conflict(
            "The replacement record is inconsistent; review it before proceeding",
        ));
    }
    Ok(data(json!({"original":original,"replacement":replacement})))
}
pub async fn replace(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(body): Json<ReplacePublicationRequest>,
) -> Result<Json<Value>> {
    auth.scope("publishing:write")?;
    let mut tx = state.db.begin().await?;
    let reference = sqlx::query("SELECT post_id,channel_id,brand_id FROM publications WHERE id=$1")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    let brand: Uuid = reference.get("brand_id");
    auth.brand(brand)?;
    let post_id: Uuid = reference.get("post_id");
    let channel: Uuid = reference.get("channel_id");
    let post = sqlx::query("SELECT * FROM posts WHERE id=$1 FOR UPDATE")
        .bind(post_id)
        .fetch_one(&mut *tx)
        .await?;
    crate::brand_admin::lock(&mut tx, brand).await?;
    crate::brand_admin::active(&mut tx, brand).await?;
    crate::schedules::lock_channel(&mut tx, channel).await?;
    let original = sqlx::query("SELECT * FROM publications WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    if original.get::<String, _>("state") != "queued"
        || original.get::<i32, _>("post_version") != body.publication_post_version
        || original.get::<chrono::DateTime<chrono::Utc>, _>("scheduled_at") != body.scheduled_at
        || post.get::<i32, _>("version") != body.post_version
    {
        return Err(ApiError::conflict(
            "Draft or queued publication changed; reload before replacing",
        ));
    }
    let payload = crate::snapshot::from_post(&mut tx, &post, channel).await?;
    crate::provider_bridge::ScheduleFormatValidator::default()
        .validate(&state, &mut tx, channel, brand, &payload)
        .await?;
    let replacement = Uuid::new_v4();
    sqlx::query("UPDATE publications SET state='canceled',last_error='Replaced with a new draft snapshot',updated_at=now() WHERE id=$1").bind(id).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO publications(id,post_id,brand_id,channel_id,post_version,scheduled_at,state,payload,scheduling_mode) VALUES($1,$2,$3,$4,$5,$6,'queued',$7,$8)").bind(replacement).bind(post_id).bind(brand).bind(channel).bind(body.post_version).bind(body.scheduled_at).bind(payload).bind(original.get::<String,_>("scheduling_mode")).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO audit_events(id,owner_id,action,subject_id,detail) VALUES($1,$2,'replace_queued_content',$3,$4)").bind(Uuid::new_v4()).bind(auth.owner_id).bind(id).bind(json!({"replacement_id":replacement,"post_version":body.post_version})).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(data(
        json!({"previous_publication_id":id,"publication":crate::publications::get_publication(&state.db,replacement).await?}),
    ))
}
pub async fn prioritize(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(body): Json<PrioritizeRequest>,
) -> Result<Json<Value>> {
    auth.scope("publishing:write")?;
    let mut tx = state.db.begin().await?;
    let record = sqlx::query("SELECT channel_id,brand_id FROM publications WHERE id=$1")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    let channel: Uuid = record.get("channel_id");
    let brand: Uuid = record.get("brand_id");
    auth.brand(brand)?;
    crate::schedules::lock_channel(&mut tx, channel).await?;
    crate::brand_admin::active(&mut tx, brand).await?;
    let rows=sqlx::query("SELECT id,scheduled_at FROM publications WHERE channel_id=$1 AND state='queued' AND scheduling_mode='queue' ORDER BY scheduled_at,id FOR UPDATE").bind(channel).fetch_all(&mut *tx).await?;
    let Some(target) = rows.iter().find(|r| r.get::<Uuid, _>("id") == id) else {
        return Err(ApiError::conflict(
            "Only queued queue-slot publications can be prioritized",
        ));
    };
    if target.get::<chrono::DateTime<chrono::Utc>, _>("scheduled_at") != body.scheduled_at {
        return Err(ApiError::conflict(
            "Queue position changed; reload before prioritizing",
        ));
    }
    let mut order = vec![id];
    order.extend(
        rows.iter()
            .map(|r| r.get::<Uuid, _>("id"))
            .filter(|item| *item != id),
    );
    sqlx::query("UPDATE publications SET state='canceled' WHERE id=ANY($1)")
        .bind(&order)
        .execute(&mut *tx)
        .await?;
    for (id, slot) in order.iter().zip(rows.iter()) {
        sqlx::query(
            "UPDATE publications SET state='queued',scheduled_at=$2,updated_at=now() WHERE id=$1",
        )
        .bind(id)
        .bind(slot.get::<chrono::DateTime<chrono::Utc>, _>("scheduled_at"))
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    let mut result = Vec::new();
    for id in order {
        result.push(crate::publications::get_publication(&state.db, id).await?);
    }
    Ok(data(result))
}
