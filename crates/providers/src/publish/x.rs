use super::*;

const X_CHUNK: u64 = 4 * 1024 * 1024;

/// Thread replies are part of the immutable publication snapshot. Keep the
/// format intentionally narrow: root media is handled by the existing path.
pub(super) fn x_thread_replies(p: &Publication) -> Result<Option<Vec<&str>>> {
    let Some(thread) = p.options.get("x_thread") else {
        return Ok(None);
    };
    let object = thread
        .as_object()
        .ok_or_else(|| invalid("X thread must contain an ordered replies array"))?;
    if object.len() != 1 || !object.contains_key("replies") {
        return Err(invalid("X thread accepts only an ordered replies array"));
    }
    let replies = thread["replies"]
        .as_array()
        .ok_or_else(|| invalid("X thread replies must be an array"))?;
    if !(1..=9).contains(&replies.len()) {
        return Err(invalid("X thread needs 1–9 text-only replies"));
    }
    replies
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|text| !text.trim().is_empty() && text.chars().count() <= 280)
                .ok_or_else(|| invalid("Each X thread reply needs 1–280 characters of text"))
        })
        .collect::<Result<Vec<_>>>()
        .map(Some)
}

fn uncertain_x(message: &str) -> ProviderError {
    error(FailureKind::Uncertain, message)
}

fn x_data(value: &Value) -> Result<&Value> {
    if value.get("errors").is_some_and(|errors| {
        !errors.is_null() && errors.as_array().is_none_or(|items| !items.is_empty())
    }) {
        return Err(uncertain_x(
            "X returned an incomplete media response; inspect the upload before recovery",
        ));
    }
    value
        .get("data")
        .filter(|data| data.is_object())
        .ok_or_else(|| {
            uncertain_x(
                "X omitted required media response data; inspect the upload before recovery",
            )
        })
}

fn x_id(value: &Value) -> Result<String> {
    let id = field(value, "id")?;
    if id.is_empty() || id.len() > 19 || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(uncertain_x(
            "X returned an invalid media ID; inspect the upload before recovery",
        ));
    }
    Ok(id)
}

fn x_expiry(value: &Value) -> Result<u64> {
    value["expires_after_secs"]
        .as_u64()
        .filter(|seconds| *seconds > 0)
        .map(|seconds| seconds.min(30 * 24 * 60 * 60))
        .ok_or_else(|| {
            uncertain_x(
                "X did not provide a usable media expiry; inspect the upload before recovery",
            )
        })
}

fn x_checkpoint_id(cp: &Value) -> Result<String> {
    x_id(cp)
}

fn valid_x_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 19 && id.bytes().all(|b| b.is_ascii_digit())
}

fn x_post_media<'a>(p: &Publication, cp: &'a Value) -> Result<&'a Vec<Value>> {
    let ids = cp["media_ids"]
        .as_array()
        .ok_or_else(|| uncertain_x("Invalid X post checkpoint"))?;
    if ids.len() != p.media.len()
        || ids
            .iter()
            .any(|id| id.as_str().is_none_or(|id| !valid_x_id(id)))
    {
        return Err(uncertain_x("Invalid X post media checkpoint"));
    }
    if p.media.first().is_some_and(video)
        && (cp["id"] != ids[0]
            || cp["total_bytes"].as_u64() != Some(p.media[0].source.len())
            || cp["offset"].as_u64() != Some(p.media[0].source.len()))
    {
        return Err(uncertain_x("Invalid X video post checkpoint"));
    }
    Ok(ids)
}

fn x_processing(data: &Value, expected: &str, cp: &Value, finalizing: bool) -> Result<Step> {
    if x_id(data)? != expected {
        return Err(uncertain_x(
            "X returned a different media ID; inspect the upload before recovery",
        ));
    }
    let info = &data["processing_info"];
    if info.is_null() && finalizing {
        let mut next = cp.clone();
        next["stage"] = json!("x_video_ready");
        return Ok(Step::Checkpoint(next));
    }
    match info["state"].as_str() {
        Some("succeeded") => {
            let mut next = cp.clone();
            next["stage"] = json!("x_video_ready");
            Ok(Step::Checkpoint(next))
        }
        Some("pending" | "in_progress") => {
            let seconds = match info.get("check_after_secs") {
                None | Some(Value::Null) => 1,
                Some(value) => value
                    .as_u64()
                    .filter(|n| *n <= 86400)
                    .ok_or_else(|| uncertain_x("X returned an invalid processing delay"))?
                    .max(1),
            };
            let mut next = cp.clone();
            next["stage"] = json!("x_video_processing");
            next["check_after_secs"] = json!(seconds);
            Ok(Step::Wait(next))
        }
        Some("failed") => Err(invalid(
            "X could not process the video; no post was created",
        )),
        _ => Err(uncertain_x("X returned an unknown video processing state")),
    }
}

impl Adapter {
    async fn x_metadata(&self, token: Option<&str>, media_id: &str, text: &str) -> Result<()> {
        let value = self
            .send(
                self.request(Method::POST, "https://api.x.com/2/media/metadata", token)?
                    .json(&json!({"id":media_id,"metadata":{"alt_text":{"text":text}}})),
                true,
            )
            .await?;
        let data = x_data(&value)?;
        if x_id(data)? != media_id {
            return Err(uncertain_x(
                "X returned a different media ID for its description",
            ));
        }
        Ok(())
    }

    pub(super) async fn publish_x(&self, c: &Channel, p: &Publication, cp: &Value) -> Result<Step> {
        let token = Some(c.tokens.access_token.as_str());
        let stage = checkpoint_stage(cp);
        if stage == "x_legacy_ambiguous" {
            return Err(uncertain_x(
                "An older X media attempt may have created a post. Inspect X before resolving it.",
            ));
        }
        if matches!(
            stage,
            "x_thread_ready" | "x_thread_post_started" | "x_thread_complete"
        ) {
            return self.publish_x_thread(token, p, cp).await;
        }
        if stage == "x_post_started" {
            let ids = x_post_media(p, cp)?;
            if x_thread_replies(p)?.is_some() {
                let mut next = cp.clone();
                next["stage"] = json!("x_thread_post_started");
                next["confirmed_ids"] = json!([]);
                next["next_index"] = json!(0);
                return Ok(Step::Checkpoint(next));
            }
            let mut body = json!({"text":p.text});
            if !ids.is_empty() {
                body["media"] = json!({"media_ids":ids});
            }
            let value = self
                .send(
                    self.request(Method::POST, "https://api.x.com/2/tweets", token)?
                        .json(&body),
                    true,
                )
                .await?;
            let id = x_id(x_data(&value)?)?;
            return Ok(published(
                id.clone(),
                Some(format!("https://x.com/i/status/{id}")),
                json!({"id":id}),
            ));
        }
        if p.media.is_empty() {
            if !stage.is_empty() {
                return Err(uncertain_x("Invalid X text checkpoint"));
            }
            return Ok(Step::Checkpoint(
                json!({"stage":"x_post_started","media_ids":[]}),
            ));
        }
        let media = &p.media[0];
        if video(media) {
            return self.publish_x_video(token, media, cp).await;
        }
        self.publish_x_images(token, p, cp).await
    }

    async fn publish_x_thread(
        &self,
        token: Option<&str>,
        p: &Publication,
        cp: &Value,
    ) -> Result<Step> {
        let replies =
            x_thread_replies(p)?.ok_or_else(|| uncertain_x("X thread snapshot is missing"))?;
        let total = replies.len() + 1;
        let raw = cp["confirmed_ids"]
            .as_array()
            .ok_or_else(|| uncertain_x("Invalid X thread checkpoint"))?;
        let mut ids = Vec::with_capacity(raw.len());
        for raw_id in raw {
            let id = raw_id
                .as_str()
                .filter(|id| valid_x_id(id))
                .ok_or_else(|| uncertain_x("Invalid X thread post ID"))?;
            if ids.contains(&id) {
                return Err(uncertain_x("Duplicate X thread post ID"));
            }
            ids.push(id);
        }
        if ids.len() > total || cp["next_index"].as_u64() != Some(ids.len() as u64) {
            return Err(uncertain_x("Invalid X thread progress"));
        }
        match checkpoint_stage(cp) {
            "x_thread_complete" if ids.len() == total => {
                let root = ids[0].to_owned();
                Ok(published(
                    root.clone(),
                    Some(format!("https://x.com/i/status/{root}")),
                    json!({"id":root,"thread_ids":ids}),
                ))
            }
            "x_thread_ready" if !ids.is_empty() && ids.len() < total => {
                let mut next = cp.clone();
                next["stage"] = json!("x_thread_post_started");
                Ok(Step::Checkpoint(next))
            }
            "x_thread_post_started" if ids.len() < total => {
                let body = if ids.is_empty() {
                    let media_ids = x_post_media(p, cp)?;
                    let mut root = json!({"text":p.text});
                    if !media_ids.is_empty() {
                        root["media"] = json!({"media_ids":media_ids});
                    }
                    root
                } else {
                    json!({"text":replies[ids.len()-1],"reply":{"in_reply_to_tweet_id":ids[ids.len()-1]}})
                };
                let value = self
                    .send(
                        self.request(Method::POST, "https://api.x.com/2/tweets", token)?
                            .json(&body),
                        true,
                    )
                    .await?;
                let id = x_id(x_data(&value)?)?;
                if ids.contains(&id.as_str()) {
                    return Err(uncertain_x("X returned a duplicate thread post ID"));
                }
                // The caller commits this checkpoint before it can issue the
                // next reply, even for the final successful response.
                let mut confirmed: Vec<String> = ids.iter().map(|id| (*id).to_owned()).collect();
                confirmed.push(id);
                let mut next = cp.clone();
                next["next_index"] = json!(confirmed.len());
                next["confirmed_ids"] = json!(confirmed);
                next["stage"] = json!(if confirmed.len() == total {
                    "x_thread_complete"
                } else {
                    "x_thread_ready"
                });
                Ok(Step::Checkpoint(next))
            }
            _ => Err(uncertain_x("Invalid X thread checkpoint state")),
        }
    }

    async fn publish_x_images(
        &self,
        token: Option<&str>,
        p: &Publication,
        cp: &Value,
    ) -> Result<Step> {
        let stage = checkpoint_stage(cp);
        if stage.is_empty() {
            return Ok(Step::Checkpoint(
                json!({"stage":"x_image_media","media_ids":[],"alt_done":[]}),
            ));
        }
        if stage != "x_image_media" {
            return Err(uncertain_x("Invalid X image checkpoint"));
        }
        let mut ids = cp["media_ids"]
            .as_array()
            .cloned()
            .ok_or_else(|| uncertain_x("Invalid X image checkpoint"))?;
        let mut done = cp["alt_done"]
            .as_array()
            .cloned()
            .ok_or_else(|| uncertain_x("Invalid X image description checkpoint"))?;
        if ids.len() > p.media.len()
            || done.len() != ids.len()
            || done.iter().any(|v| !v.is_boolean())
        {
            return Err(uncertain_x("Invalid X image progress"));
        }
        for (index, id) in ids.iter().enumerate() {
            let id = id
                .as_str()
                .ok_or_else(|| uncertain_x("Invalid X image ID"))?;
            if id.is_empty() || id.len() > 19 || !id.bytes().all(|b| b.is_ascii_digit()) {
                return Err(uncertain_x("Invalid X image ID"));
            }
            if done[index] == false {
                self.x_metadata(token, id, x_alt_text(p, &p.media[index]))
                    .await?;
                done[index] = json!(true);
                let mut next = cp.clone();
                next["alt_done"] = json!(done);
                return Ok(Step::Checkpoint(next));
            }
        }
        if ids.len() < p.media.len() {
            let media = &p.media[ids.len()];
            let value = self
                .send(
                    self.request(Method::POST, "https://api.x.com/2/media/upload", token)?
                        .multipart(
                            multipart(media, "media")
                                .await?
                                .text("media_category", "tweet_image"),
                        ),
                    true,
                )
                .await?;
            let data = x_data(&value)?;
            ids.push(json!(x_id(data)?));
            done.push(json!(x_alt_text(p, media).trim().is_empty()));
            let mut next = cp.clone();
            next["media_ids"] = json!(ids);
            next["alt_done"] = json!(done);
            next["expires_after_secs"] = json!(x_expiry(data)?);
            return Ok(Step::Checkpoint(next));
        }
        let mut next = cp.clone();
        next["stage"] = json!("x_post_started");
        Ok(Step::Checkpoint(next))
    }

    async fn publish_x_video(
        &self,
        token: Option<&str>,
        media: &Media,
        cp: &Value,
    ) -> Result<Step> {
        if matches!(
            checkpoint_stage(cp),
            "x_video_finalize" | "x_video_processing" | "x_video_ready"
        ) && (cp["total_bytes"].as_u64() != Some(media.source.len())
            || cp["offset"].as_u64() != Some(media.source.len()))
        {
            return Err(uncertain_x("Invalid X completed upload checkpoint"));
        }
        match checkpoint_stage(cp) {
            "" => Ok(Step::Checkpoint(json!({"stage":"x_video_init"}))),
            "x_video_init" => {
                let value = self.send(self.request(Method::POST, "https://api.x.com/2/media/upload/initialize", token)?
                    .json(&json!({"media_type":"video/mp4","total_bytes":media.source.len(),"media_category":"tweet_video"})), true).await?;
                let data = x_data(&value)?;
                Ok(Step::Checkpoint(
                    json!({"stage":"x_video_append","id":x_id(data)?,"offset":0,"segment_index":0,"total_bytes":media.source.len(),"expires_after_secs":x_expiry(data)?}),
                ))
            }
            "x_video_append" => {
                let id = x_checkpoint_id(cp)?;
                let total = cp["total_bytes"]
                    .as_u64()
                    .filter(|n| *n == media.source.len())
                    .ok_or_else(|| uncertain_x("X upload size no longer matches the snapshot"))?;
                let offset = cp["offset"]
                    .as_u64()
                    .ok_or_else(|| uncertain_x("Invalid X upload offset"))?;
                let index = cp["segment_index"]
                    .as_u64()
                    .filter(|n| *n <= 9999)
                    .ok_or_else(|| uncertain_x("Invalid X upload segment"))?;
                if offset >= total || offset != index.saturating_mul(X_CHUNK) {
                    return Err(uncertain_x("Invalid X upload progress"));
                }
                let length = (total - offset).min(X_CHUNK);
                let part =
                    Part::stream_with_length(media.source.body(offset, length).await?, length)
                        .file_name(media.name.clone())
                        .mime_str("video/mp4")
                        .map_err(|_| invalid("Invalid media type"))?;
                let form = Form::new()
                    .text("segment_index", index.to_string())
                    .part("media", part);
                let value = self
                    .send(
                        self.request(
                            Method::POST,
                            &format!("https://api.x.com/2/media/upload/{id}/append"),
                            token,
                        )?
                        .multipart(form),
                        true,
                    )
                    .await?;
                x_data(&value)?;
                let mut next = cp.clone();
                next["offset"] = json!(offset + length);
                next["segment_index"] = json!(index + 1);
                if offset + length == total {
                    next["stage"] = json!("x_video_finalize");
                }
                Ok(Step::Checkpoint(next))
            }
            "x_video_finalize" => {
                let id = x_checkpoint_id(cp)?;
                if cp["total_bytes"].as_u64() != Some(media.source.len())
                    || cp["offset"].as_u64() != Some(media.source.len())
                {
                    return Err(uncertain_x("Invalid X finalization checkpoint"));
                }
                let value = self
                    .send(
                        self.request(
                            Method::POST,
                            &format!("https://api.x.com/2/media/upload/{id}/finalize"),
                            token,
                        )?,
                        true,
                    )
                    .await?;
                x_processing(x_data(&value)?, &id, cp, true)
            }
            "x_video_processing" => {
                let id = x_checkpoint_id(cp)?;
                let value = self
                    .send(
                        self.request(Method::GET, "https://api.x.com/2/media/upload", token)?
                            .query(&[("command", "STATUS"), ("media_id", id.as_str())]),
                        false,
                    )
                    .await
                    .map_err(|error| {
                        if error.kind == FailureKind::Rejected {
                            uncertain_x("X media processing status could not be verified")
                        } else {
                            error
                        }
                    })?;
                x_processing(x_data(&value)?, &id, cp, false)
            }
            "x_video_ready" => {
                let id = x_checkpoint_id(cp)?;
                let mut next = cp.clone();
                next["stage"] = json!("x_post_started");
                next["media_ids"] = json!([id]);
                Ok(Step::Checkpoint(next))
            }
            _ => Err(uncertain_x("Invalid X video checkpoint")),
        }
    }
}
