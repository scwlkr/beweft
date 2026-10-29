use super::*;
impl Adapter {
    pub(super) async fn publish_meta(
        &self,
        c: &Channel,
        p: &Publication,
        cp: &Value,
    ) -> Result<Step> {
        let token = Some(c.tokens.access_token.as_str());
        let account = id_path(&c.identity.id);
        let stage = checkpoint_stage(cp);
        let ig = c.app.provider == "instagram";
        let base = if ig {
            "https://graph.instagram.com/v23.0"
        } else {
            "https://graph.threads.net/v1.0"
        };
        let edge = if ig { "media" } else { "threads" };
        if stage.is_empty() {
            let mut body = if ig {
                json!({"caption":p.text})
            } else {
                json!({"text":p.text,"media_type":"TEXT"})
            };
            if let Some(m) = p.media.first() {
                if video(m) {
                    body["media_type"] = json!(if ig { "REELS" } else { "VIDEO" });
                    body["video_url"] = json!(public_media(m)?);
                } else {
                    if !ig {
                        body["media_type"] = json!("IMAGE");
                    }
                    body["image_url"] = json!(public_media(m)?);
                    if ig && !m.alt_text.is_empty() {
                        body["alt_text"] = json!(m.alt_text);
                    }
                }
            }
            if !ig && let Some(reply) = p.options["reply_control"].as_str() {
                if !matches!(reply, "everyone" | "accounts_you_follow" | "mentioned_only") {
                    return Err(invalid("Unsupported Threads reply control"));
                }
                body["reply_control"] = json!(reply);
            }
            let v = self
                .send(
                    self.request(Method::POST, &format!("{base}/{account}/{edge}"), token)?
                        .json(&body),
                    true,
                )
                .await?;
            return Ok(Step::Checkpoint(
                json!({"stage":"container","container_id":field(&v,"id")?}),
            ));
        }
        let container = field(cp, "container_id")?;
        if stage == "container" {
            let v = self
                .send(
                    self.request(
                        Method::GET,
                        &format!("{base}/{}", id_path(&container)),
                        token,
                    )?
                    .query(&[("fields", if ig { "status_code" } else { "status" })]),
                    false,
                )
                .await?;
            let status = v[if ig { "status_code" } else { "status" }]
                .as_str()
                .unwrap_or("");
            return match status {
                "FINISHED" => Ok(Step::Checkpoint(
                    json!({"stage":"ready","container_id":container}),
                )),
                "IN_PROGRESS" => Ok(Step::Wait(cp.clone())),
                "PUBLISHED" => Err(error(
                    FailureKind::Uncertain,
                    "Container is already published; reconcile its publication ID",
                )),
                "ERROR" | "EXPIRED" => Err(invalid("Provider media processing failed or expired")),
                _ => Err(error(
                    FailureKind::Uncertain,
                    "Provider returned an unknown processing status",
                )),
            };
        }
        let v = self
            .send(
                self.request(
                    Method::POST,
                    &format!(
                        "{base}/{account}/{}_publish",
                        if ig { "media" } else { "threads" }
                    ),
                    token,
                )?
                .json(&json!({"creation_id":container})),
                true,
            )
            .await?;
        let id = field(&v, "id")?;
        // The publication ID is authoritative even if a subsequent permalink read fails.
        Ok(published(
            id.clone(),
            None,
            json!({"id":id,"container_id":container}),
        ))
    }
}
