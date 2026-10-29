use super::*;
impl Adapter {
    pub(super) async fn publish_mastodon(
        &self,
        c: &Channel,
        p: &Publication,
        cp: &Value,
    ) -> Result<Step> {
        let token = Some(c.tokens.access_token.as_str());
        let origin = instance_origin(&c.app)?;
        let mut ids = cp["media_ids"].as_array().cloned().unwrap_or_default();
        if let Some(id) = cp["pending_media"].as_str() {
            let v = self
                .send(
                    self.request(
                        Method::GET,
                        &format!("{origin}/api/v1/media/{}", id_path(id)),
                        token,
                    )?,
                    false,
                )
                .await?;
            if v["url"].is_null() {
                return Ok(Step::Wait(cp.clone()));
            }
            ids.push(json!(id));
            return Ok(Step::Checkpoint(json!({"stage":"media","media_ids":ids})));
        }
        if ids.len() < p.media.len() {
            let m = &p.media[ids.len()];
            let v = self
                .send(
                    self.request(Method::POST, &format!("{origin}/api/v2/media"), token)?
                        .multipart(
                            multipart(m, "file")
                                .await?
                                .text("description", m.alt_text.clone()),
                        ),
                    true,
                )
                .await?;
            let id = field(&v, "id")?;
            if v["url"].is_null() {
                return Ok(Step::Checkpoint(
                    json!({"stage":"media","media_ids":ids,"pending_media":id}),
                ));
            }
            ids.push(json!(id));
            return Ok(Step::Checkpoint(json!({"stage":"media","media_ids":ids})));
        }
        let visibility = p.options["visibility"].as_str().unwrap_or("public");
        if !matches!(visibility, "public" | "unlisted" | "private" | "direct") {
            return Err(invalid("Invalid Mastodon visibility"));
        }
        let body = json!({"status":p.text,"media_ids":ids,"visibility":visibility,"spoiler_text":p.options["content_warning"].as_str().unwrap_or(""),"sensitive":p.options["sensitive"].as_bool().unwrap_or(false)});
        let v = self
            .send(
                self.request(Method::POST, &format!("{origin}/api/v1/statuses"), token)?
                    .header("Idempotency-Key", &p.key)
                    .json(&body),
                true,
            )
            .await?;
        let id = field(&v, "id")?;
        Ok(published(
            id.clone(),
            v["url"].as_str().map(str::to_owned),
            json!({"id":id}),
        ))
    }
}
