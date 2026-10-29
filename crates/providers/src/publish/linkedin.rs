use super::*;
impl Adapter {
    pub(super) async fn publish_linkedin(
        &self,
        c: &Channel,
        p: &Publication,
        _cp: &Value,
    ) -> Result<Step> {
        let token = Some(c.tokens.access_token.as_str());
        let body = json!({"author":format!("urn:li:person:{}",c.identity.id),"commentary":p.text,"visibility":"PUBLIC","distribution":{"feedDistribution":"MAIN_FEED","targetEntities":[],"thirdPartyDistributionChannels":[]},"lifecycleState":"PUBLISHED","isReshareDisabledByAuthor":false});
        let response = self
            .response(
                self.request(Method::POST, "https://api.linkedin.com/rest/posts", token)?
                    .header("Linkedin-Version", "202610")
                    .header("X-Restli-Protocol-Version", "2.0.0")
                    .json(&body),
                true,
            )
            .await?;
        let id = response
            .headers()
            .get("x-restli-id")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                error(
                    FailureKind::Uncertain,
                    "LinkedIn response did not include a publication receipt",
                )
            })?
            .to_owned();
        Ok(published(
            id.clone(),
            Some(format!("https://www.linkedin.com/feed/update/{id}")),
            json!({"id":id}),
        ))
    }
}
