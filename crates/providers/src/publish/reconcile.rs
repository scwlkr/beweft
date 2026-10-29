use super::*;
impl Adapter {
    /// Only reads/status probes. Never create a new publication during reconciliation.
    pub async fn reconcile(
        &self,
        c: &Channel,
        p: &Publication,
        cp: &Value,
    ) -> Result<Option<Step>> {
        let token = Some(c.tokens.access_token.as_str());
        match c.app.provider.as_str() {
            "youtube" => {
                let session = field(cp, "session")?;
                let b = self
                    .request(Method::PUT, &session, token)?
                    .header("Content-Length", "0")
                    .header(
                        "Content-Range",
                        format!("bytes */{}", p.media[0].source.len()),
                    );
                let r = self.raw_response(b, false).await?;
                if r.status().as_u16() == 308 {
                    let offset = received_offset(&r)?;
                    if offset > p.media[0].source.len() {
                        return Err(error(
                            FailureKind::Uncertain,
                            "Invalid YouTube session offset",
                        ));
                    }
                    let mut next = cp.clone();
                    next["offset"] = json!(offset);
                    return Ok(Some(Step::Checkpoint(next)));
                }
                if r.status().as_u16() == 404 {
                    return Err(error(
                        FailureKind::ExpiredSession,
                        "YouTube upload session expired. Inspect YouTube Studio to establish the outcome, record owner reconciliation, then explicitly reset the expired session.",
                    ));
                }
                if !r.status().is_success() {
                    return Ok(None);
                }
                let v: Value = r
                    .json()
                    .await
                    .map_err(|_| invalid("Invalid upload status response"))?;
                let id = field(&v, "id")?;
                Ok(Some(published(
                    id.clone(),
                    Some(format!("https://www.youtube.com/watch?v={id}")),
                    json!({"id":id,"reconciled":true}),
                )))
            }
            "bluesky" => {
                let rkey = field(cp, "rkey")?;
                let origin = instance_origin(&c.app)?;
                let v = self
                    .send(
                        self.request(
                            Method::GET,
                            &format!("{origin}/xrpc/com.atproto.repo.getRecord"),
                            token,
                        )?
                        .query(&[
                            ("repo", c.identity.id.as_str()),
                            ("collection", "app.bsky.feed.post"),
                            ("rkey", rkey.as_str()),
                        ]),
                        false,
                    )
                    .await?;
                if v["value"]["text"].as_str() != Some(p.text.as_str()) {
                    return Err(error(
                        FailureKind::Uncertain,
                        "Existing Bluesky record does not match the publication snapshot",
                    ));
                }
                let id = field(&v, "uri")?;
                Ok(Some(published(
                    id.clone(),
                    Some(format!(
                        "https://bsky.app/profile/{}/post/{rkey}",
                        c.identity.id
                    )),
                    json!({"uri":id,"cid":v["cid"],"reconciled":true}),
                )))
            }
            _ => Ok(None),
        }
    }
}
