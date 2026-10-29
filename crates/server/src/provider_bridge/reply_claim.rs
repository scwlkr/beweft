use super::*;
pub(super) async fn check_history(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    auth: &Auth,
    id: Uuid,
    request_id: Uuid,
    body: &Value,
) -> Result<Option<Value>> {
    sqlx::query("UPDATE provider_replies SET state='uncertain',error='Reply response was lost; inspect the network and record its outcome before a new reply',updated_at=now() WHERE inbox_id=$1 AND state='sending' AND delivery_deadline<clock_timestamp()").bind(id).execute(&mut **tx).await?;
    let existing:Option<Value>=sqlx::query_scalar("SELECT to_jsonb(r)-'owner_id'-'history_order'-'delivery_deadline' FROM provider_replies r WHERE id=$1 AND inbox_id=$2 AND owner_id=$3").bind(request_id).bind(id).bind(auth.owner_id).fetch_optional(&mut **tx).await?;
    if let Some(value) = existing {
        return Ok(Some(value));
    }
    let expected: Option<Uuid> = match body.get("expected_last_request_id") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => Some(
            value
                .parse()
                .map_err(|_| ApiError::bad("expected_last_request_id must be a UUID or null"))?,
        ),
        _ => {
            return Err(ApiError::bad(
                "expected_last_request_id must be a UUID or null",
            ));
        }
    };
    let latest: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM provider_replies WHERE inbox_id=$1 ORDER BY history_order DESC LIMIT 1",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?;
    if latest != expected {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "reply_history_changed",
            "Reply history changed. Reload the conversation and explicitly confirm another reply."
                .into(),
        ));
    }
    let unresolved:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM provider_replies WHERE inbox_id=$1 AND state IN ('sending','uncertain'))").bind(id).fetch_one(&mut **tx).await?;
    if unresolved {
        return Err(ApiError::conflict(
            "An earlier reply is still sending or uncertain. Inspect its durable result and resolve its outcome before a new reply.",
        ));
    }
    Ok(None)
}
