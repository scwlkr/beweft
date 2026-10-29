use super::*;

fn facebook_video_id(cp: &Value) -> Result<String> {
    let id = field(cp, "video_id")?;
    if id.len() > 32 || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(error(
            FailureKind::Uncertain,
            "Invalid Facebook Reel checkpoint; inspect the provider before retrying",
        ));
    }
    Ok(id)
}

fn facebook_checkpoint(stage: &str, id: &str) -> Value {
    json!({"stage":stage,"video_id":id,"expires_after_secs":3600,"check_after_secs":60})
}

impl Adapter {
    pub(super) async fn publish_facebook(
        &self,
        c: &Channel,
        p: &Publication,
        cp: &Value,
    ) -> Result<Step> {
        if p.options["format"] == "reel" {
            return self.publish_facebook_reel(c, p, cp).await;
        }
        if !checkpoint_stage(cp).is_empty() {
            return Err(error(
                FailureKind::Uncertain,
                "Invalid Facebook delivery checkpoint",
            ));
        }
        let token = Some(c.tokens.access_token.as_str());
        let account = id_path(&c.identity.id);
        let url = if p.media.is_empty() {
            format!("https://graph.facebook.com/v23.0/{account}/feed")
        } else {
            format!("https://graph.facebook.com/v23.0/{account}/photos")
        };
        let body = if p.media.is_empty() {
            json!({"message":p.text})
        } else {
            json!({"caption":p.text,"url":public_media(&p.media[0])?,"published":true})
        };
        let v = self
            .send(self.request(Method::POST, &url, token)?.json(&body), true)
            .await?;
        let id = match v["post_id"].as_str().filter(|id| !id.is_empty()) {
            Some(id) => id.to_owned(),
            None => field(&v, "id")?,
        };
        Ok(published(
            id.clone(),
            Some(format!("https://www.facebook.com/{id}")),
            json!({"id":id}),
        ))
    }

    async fn publish_facebook_reel(
        &self,
        c: &Channel,
        p: &Publication,
        cp: &Value,
    ) -> Result<Step> {
        let token = Some(c.tokens.access_token.as_str());
        let stage = checkpoint_stage(cp);
        if stage.is_empty() {
            let v = self
                .send(
                    self.request(
                        Method::POST,
                        "https://graph.facebook.com/v23.0/me/video_reels",
                        token,
                    )?
                    .form(&[("upload_phase", "start")]),
                    true,
                )
                .await?;
            let id = field(&v, "video_id")?;
            facebook_video_id(&json!({"video_id":id}))?;
            let upload = field(&v, "upload_url")?;
            let expected = format!("https://rupload.facebook.com/video-upload/v23.0/{id}");
            if upload != expected {
                return Err(error(
                    FailureKind::Uncertain,
                    "Facebook returned an unexpected upload endpoint; inspect the delivery before retrying",
                ));
            }
            return Ok(Step::Checkpoint(facebook_checkpoint("fb_reel_upload", &id)));
        }
        if !matches!(
            stage,
            "fb_reel_upload" | "fb_reel_uploaded" | "fb_reel_public" | "fb_reel_processing"
        ) {
            return Err(error(
                FailureKind::Uncertain,
                "Invalid Facebook Reel checkpoint",
            ));
        }
        let id = facebook_video_id(cp)?;
        if stage == "fb_reel_upload" {
            let media = &p.media[0];
            let url = format!("https://rupload.facebook.com/video-upload/v23.0/{id}");
            let v = self
                .send(
                    self.request(Method::POST, &url, None)?
                        .header("Authorization", format!("OAuth {}", c.tokens.access_token))
                        .header("offset", "0")
                        .header("file_size", media.source.len())
                        .header("Content-Type", "application/octet-stream")
                        .header("Content-Length", media.source.len())
                        .body(media.source.body(0, media.source.len()).await?),
                    true,
                )
                .await?;
            if v["success"] != true {
                return Err(error(
                    FailureKind::Uncertain,
                    "Facebook did not confirm its Reel upload",
                ));
            }
            return Ok(Step::Checkpoint(facebook_checkpoint(
                "fb_reel_uploaded",
                &id,
            )));
        }
        if stage == "fb_reel_uploaded" {
            return Ok(Step::Checkpoint(facebook_checkpoint("fb_reel_public", &id)));
        }
        if stage == "fb_reel_public" {
            let v = self
                .send(
                    self.request(
                        Method::POST,
                        "https://graph.facebook.com/v23.0/me/video_reels",
                        token,
                    )?
                    .form(&[
                        ("upload_phase", "finish"),
                        ("video_id", id.as_str()),
                        ("video_state", "PUBLISHED"),
                        ("description", p.text.as_str()),
                        ("title", p.title.as_str()),
                    ]),
                    true,
                )
                .await?;
            if v["success"] != true {
                return Err(error(
                    FailureKind::Uncertain,
                    "Facebook did not confirm its Reel publish request",
                ));
            }
            return Ok(Step::Checkpoint(facebook_checkpoint(
                "fb_reel_processing",
                &id,
            )));
        }
        self.facebook_reel_status(c, &id, cp).await
    }

    pub(super) async fn facebook_reel_status(
        &self,
        c: &Channel,
        id: &str,
        cp: &Value,
    ) -> Result<Step> {
        let v = self
            .send(
                self.request(
                    Method::GET,
                    &format!("https://graph.facebook.com/v23.0/{id}"),
                    Some(&c.tokens.access_token),
                )?
                .query(&[("fields", "status")]),
                false,
            )
            .await?;
        if v["id"].as_str() != Some(id) {
            return Err(error(
                FailureKind::Uncertain,
                "Facebook returned a different Reel status identifier",
            ));
        }
        let status = &v["status"];
        let publishing = status["publishing_phase"]["status"].as_str().unwrap_or("");
        if matches!(
            status["video_status"].as_str(),
            Some("error" | "failed" | "upload_failed" | "expired")
        ) || matches!(
            status["processing_phase"]["status"].as_str(),
            Some("error" | "failed")
        ) || matches!(
            status["uploading_phase"]["status"].as_str(),
            Some("error" | "failed")
        ) || matches!(publishing, "error" | "failed")
        {
            return Err(error(
                FailureKind::Uncertain,
                "Facebook reported Reel processing or publishing failure; inspect the outcome before retrying",
            ));
        }
        if publishing == "complete"
            && status["publishing_phase"]["publish_status"] == "published"
            && status["video_status"] == "ready"
            && status["processing_phase"]["status"] == "complete"
        {
            return Ok(published(
                id.to_owned(),
                None,
                json!({"id":id,"status":"publishing_complete","video_status":"ready","processing_status":"complete","publishing_status":"complete","publish_status":"published"}),
            ));
        }
        if matches!(publishing, "not_started" | "processing" | "in_progress")
            || (publishing == "complete"
                && status["publishing_phase"]["publish_status"] == "published"
                && (matches!(
                    status["video_status"].as_str(),
                    Some("processing" | "uploading" | "upload_complete")
                ) || matches!(
                    status["processing_phase"]["status"].as_str(),
                    Some("not_started" | "in_progress" | "processing")
                )))
        {
            let polls = cp["polls"].as_u64().unwrap_or(0);
            if polls >= 9 {
                return Err(error(
                    FailureKind::Uncertain,
                    "Facebook Reel processing did not finish within the supported polling window. Inspect its outcome before retrying.",
                ));
            }
            let mut wait = cp.clone();
            wait["polls"] = json!(polls + 1);
            wait["check_after_secs"] = json!(60);
            return Ok(Step::Wait(wait));
        }
        Err(error(
            FailureKind::Uncertain,
            "Facebook has not confirmed the Reel was published; inspect its status before retrying",
        ))
    }
}
