use super::*;

/// Presence, even malformed, identifies an X thread for fail-closed recovery.
pub(crate) fn has_x_thread(payload: &Value) -> bool {
    payload["options"].get("x_thread").is_some()
}

fn frozen_texts(payload: &Value) -> Result<Vec<String>> {
    let thread = &payload["options"]["x_thread"];
    let object = thread
        .as_object()
        .filter(|o| o.len() == 1 && o.contains_key("replies"))
        .ok_or_else(|| {
            ApiError::conflict("The saved X thread is invalid; recovery cannot proceed")
        })?;
    let replies = object["replies"]
        .as_array()
        .filter(|r| (1..=9).contains(&r.len()))
        .ok_or_else(|| ApiError::conflict("The saved X thread replies are invalid"))?;
    let root = payload["text"]
        .as_str()
        .ok_or_else(|| ApiError::conflict("The saved X thread root is invalid"))?;
    let mut texts = vec![root.to_owned()];
    for reply in replies {
        let text = reply
            .as_str()
            .filter(|s| !s.trim().is_empty() && s.chars().count() <= 280)
            .ok_or_else(|| ApiError::conflict("The saved X thread reply is invalid"))?;
        texts.push(text.to_owned());
    }
    Ok(texts)
}

pub(crate) fn valid_x_thread_snapshot(payload: &Value) -> bool {
    frozen_texts(payload).is_ok()
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 19 && id.bytes().all(|b| b.is_ascii_digit())
}

/// Only a recognized, unambiguous private-upload checkpoint may use the
/// existing reset flow. Missing and malformed checkpoints never prove absence.
pub(crate) fn x_thread_pre_public(cp: &Value) -> bool {
    !super::reconciliation::x_public_boundary(cp)
        && cp.get("confirmed_ids").is_none()
        && cp.get("next_index").is_none()
}

/// A thread with no confirmed root can be reset only after the owner has
/// attested that this exact remote root operation definitively ended without
/// creating a post. This is an owner observation, not provider proof.
pub(crate) fn x_thread_root_ended_attested(payload: &Value, cp: &Value) -> Result<bool> {
    let status = x_thread_status(payload, cp)?;
    Ok(status["phase"] == "request_started"
        && status["next_index"] == 0
        && status["confirmed_ids"]
            .as_array()
            .is_some_and(Vec::is_empty)
        && cp["not_published_attestation"]["attempt_id"] == status["attempt_id"]
        && cp["not_published_attestation"]["checkpoint_id"] == status["checkpoint_id"]
        && cp["thread_root_ended_attestation"]["attempt_id"] == status["attempt_id"]
        && cp["thread_root_ended_attestation"]["checkpoint_id"] == status["checkpoint_id"])
}

pub(crate) async fn x_thread_reset_covers_attempts(
    db: &mut sqlx::PgConnection,
    id: Uuid,
) -> Result<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM audit_events r WHERE r.subject_id=$1 AND r.action='reset_x_upload' AND r.detail->'thread_safe_reset'='true'::jsonb AND jsonb_typeof(r.detail->'attempt_ids')='array' AND NOT EXISTS(SELECT 1 FROM publication_attempts a WHERE a.publication_id=$1 AND NOT (r.detail->'attempt_ids' ? a.id::text)))")
        .bind(id).fetch_one(db).await?)
}

pub(crate) fn x_thread_status(payload: &Value, cp: &Value) -> Result<Value> {
    let texts = frozen_texts(payload)?;
    let (attempt, checkpoint) = super::reconciliation::x_identity(cp)?;
    let stage = cp["stage"].as_str().unwrap_or("");
    let phase = if x_thread_pre_public(cp) {
        "preparing"
    } else {
        match stage {
            "x_post_started" | "x_legacy_ambiguous" | "x_thread_post_started" => "request_started",
            "x_thread_ready" => "ready",
            "x_thread_complete" => "complete",
            _ => {
                return Err(ApiError::conflict(
                    "The X thread checkpoint is invalid; inspect the thread before recovery",
                ));
            }
        }
    };
    let raw = match cp.get("confirmed_ids") {
        None if matches!(stage, "x_post_started" | "x_legacy_ambiguous")
            || phase == "preparing" =>
        {
            vec![]
        }
        Some(Value::Array(ids)) => ids.clone(),
        _ => {
            return Err(ApiError::conflict(
                "The X thread checkpoint has invalid confirmed IDs",
            ));
        }
    };
    let mut ids: Vec<String> = Vec::with_capacity(raw.len());
    for raw_id in raw {
        let id = raw_id
            .as_str()
            .filter(|id| valid_id(id))
            .ok_or_else(|| ApiError::conflict("The X thread checkpoint has an invalid ID"))?;
        if ids.iter().any(|existing| existing == id) {
            return Err(ApiError::conflict(
                "The X thread checkpoint contains duplicate IDs",
            ));
        }
        ids.push(id.to_owned());
    }
    let index = ids.len();
    let progress_valid = match phase {
        "preparing" => index == 0,
        "ready" => index > 0 && index < texts.len(),
        "request_started" => index < texts.len(),
        "complete" => index == texts.len(),
        _ => false,
    };
    let index_valid = match cp.get("next_index") {
        Some(value) => value.as_u64() == Some(index as u64),
        None => matches!(stage, "x_post_started" | "x_legacy_ambiguous") || phase == "preparing",
    };
    if !progress_valid || !index_valid {
        return Err(ApiError::conflict(
            "The X thread checkpoint has inconsistent progress",
        ));
    }
    Ok(
        json!({"phase":phase,"confirmed_ids":ids,"next_index":index,"total":texts.len(),"remaining_texts":texts[index..],"attempt_id":attempt,"checkpoint_id":checkpoint}),
    )
}

pub(crate) fn x_thread_completion(
    payload: &Value,
    cp: &Value,
    body: &Value,
) -> Result<(String, String)> {
    let status = x_thread_status(payload, cp)?;
    if body["observed_attempt_id"] != status["attempt_id"]
        || body["observed_checkpoint_id"] != status["checkpoint_id"]
        || body["sender_stopped"] != true
        || body["provider_inspected"] != true
    {
        return Err(ApiError::conflict(
            "Stop the sender, inspect the complete X reply chain, and use the current attempt and checkpoint IDs",
        ));
    }
    let supplied = body["thread_ids"]
        .as_array()
        .ok_or_else(|| ApiError::bad("Provide the complete ordered X thread IDs"))?;
    let prefix = status["confirmed_ids"]
        .as_array()
        .expect("status constructs an array");
    if supplied.len() != status["total"].as_u64().unwrap_or(0) as usize
        || !supplied.starts_with(prefix)
    {
        return Err(ApiError::conflict(
            "Provide every X thread ID in order, preserving the confirmed prefix",
        ));
    }
    let mut ids: Vec<&str> = Vec::with_capacity(supplied.len());
    for value in supplied {
        let id = value.as_str().filter(|id| valid_id(id)).ok_or_else(|| {
            ApiError::bad("X thread IDs must be numeric IDs of at most 19 digits")
        })?;
        if ids.contains(&id) {
            return Err(ApiError::bad("X thread IDs must be unique"));
        }
        ids.push(id);
    }
    let root = ids[0].to_owned();
    let url = format!("https://x.com/i/status/{root}");
    if body.get("external_id").is_some_and(|id| id != &json!(root))
        || body
            .get("external_url")
            .is_some_and(|external| external != &json!(url))
    {
        return Err(ApiError::bad(
            "The root ID and URL must match the first X thread ID",
        ));
    }
    Ok((root, url))
}
