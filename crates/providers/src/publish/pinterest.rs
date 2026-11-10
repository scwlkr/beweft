use super::*;
impl Adapter {
    pub(super) async fn publish_pinterest(
        &self,
        c: &Channel,
        p: &Publication,
        _cp: &Value,
    ) -> Result<Step> {
        let media_source = if p.media.len() == 1 {
            json!({
                "source_type": "image_url",
                "url": public_media(&p.media[0])?
            })
        } else {
            json!({
                "source_type": "multiple_image_urls",
                "items": p
                    .media
                    .iter()
                    .map(|m| Ok(json!({"url": public_media(m)?})))
                    .collect::<Result<Vec<_>>>()?
            })
        };
        let body = json!({
            "board_id": p.options["board_id"],
            "title": p.title,
            "description": p.text,
            "link": p.options["link"],
            "media_source": media_source
        });
        let v = self
            .send(
                self.request(
                    Method::POST,
                    "https://api.pinterest.com/v5/pins",
                    Some(&c.tokens.access_token),
                )?
                .json(&body),
                true,
            )
            .await?;
        let id = field(&v, "id")?;
        Ok(published(
            id.clone(),
            Some(format!("https://www.pinterest.com/pin/{id}/")),
            json!({"id":id}),
        ))
    }
}