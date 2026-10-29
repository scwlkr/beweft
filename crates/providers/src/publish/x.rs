use super::*;
impl Adapter {
    pub(super) async fn publish_x(&self, c: &Channel, p: &Publication, cp: &Value) -> Result<Step> {
        let token = Some(c.tokens.access_token.as_str());
        let mut ids = cp["media_ids"].as_array().cloned().unwrap_or_default();
        if ids.len() < p.media.len() {
            let v = self
                .send(
                    self.request(Method::POST, "https://api.x.com/2/media/upload", token)?
                        .multipart(
                            multipart(&p.media[ids.len()], "media")
                                .await?
                                .text("media_category", "tweet_image"),
                        ),
                    true,
                )
                .await?;
            let id = field(&v["data"], "id")?;
            ids.push(json!(id));
            return Ok(Step::Checkpoint(json!({"stage":"media","media_ids":ids})));
        }
        let mut body = json!({"text":p.text});
        if !ids.is_empty() {
            body["media"] = json!({"media_ids":ids});
        }
        let v = self
            .send(
                self.request(Method::POST, "https://api.x.com/2/tweets", token)?
                    .json(&body),
                true,
            )
            .await?;
        let id = field(&v["data"], "id")?;
        Ok(published(
            id.clone(),
            Some(format!("https://x.com/i/status/{id}")),
            json!({"id":id}),
        ))
    }
}
