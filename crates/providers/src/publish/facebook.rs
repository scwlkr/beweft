use super::*;
impl Adapter {
    pub(super) async fn publish_facebook(
        &self,
        c: &Channel,
        p: &Publication,
        _cp: &Value,
    ) -> Result<Step> {
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
        let id = v["post_id"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or(field(&v, "id")?);
        Ok(published(
            id.clone(),
            Some(format!("https://www.facebook.com/{id}")),
            json!({"id":id}),
        ))
    }
}
