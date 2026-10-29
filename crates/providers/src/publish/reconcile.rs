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
                let v = self.response_json(r, false).await?;
                let id = field(&v, "id")?;
                Ok(Some(published(
                    id.clone(),
                    Some(format!("https://www.youtube.com/watch?v={id}")),
                    json!({"id":id,"reconciled":true}),
                )))
            }
            "facebook"
                if matches!(
                    checkpoint_stage(cp),
                    "fb_reel_public" | "fb_reel_processing"
                ) =>
            {
                let id = field(cp, "video_id")?;
                if id.len() > 32 || !id.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(error(
                        FailureKind::Uncertain,
                        "Invalid Facebook Reel checkpoint",
                    ));
                }
                match self.facebook_reel_status(c, &id, cp).await? {
                    Step::Published {
                        external_id,
                        external_url,
                        mut receipt,
                    } => {
                        receipt["reconciled"] = json!(true);
                        Ok(Some(Step::Published {
                            external_id,
                            external_url,
                            receipt,
                        }))
                    }
                    _ => Ok(None),
                }
            }
            "bluesky" => {
                // These stages precede the public record boundary. A saved job
                // can be polled and resumed without making another upload.
                if checkpoint_stage(cp) == "bsky_prepared" {
                    field(cp, "rkey")?;
                    field(cp, "created_at")?;
                    let blobs = cp["blobs"]
                        .as_array()
                        .filter(|blobs| blobs.len() <= p.media.len())
                        .ok_or_else(|| {
                            error(FailureKind::Uncertain, "Invalid Bluesky media checkpoint")
                        })?;
                    if blobs
                        .iter()
                        .zip(&p.media)
                        .any(|(blob, media)| !super::bluesky::valid_blob(blob, &media.mime))
                    {
                        return Err(error(
                            FailureKind::Uncertain,
                            "Invalid Bluesky media checkpoint",
                        ));
                    }
                    return Ok(Some(Step::Checkpoint(cp.clone())));
                }
                if checkpoint_stage(cp) == "bsky_video_preflight" {
                    // Verify the read succeeds before allowing a resumed worker
                    // to repeat the preflight and reach the upload boundary.
                    self.publish_bluesky(c, p, cp).await?;
                    return Ok(Some(Step::Checkpoint(cp.clone())));
                }
                if checkpoint_stage(cp) == "bsky_video_processing" {
                    require_permission(c, "media")?;
                    return match self.publish_bluesky(c, p, cp).await? {
                        Step::Wait(next) | Step::Checkpoint(next) => {
                            Ok(Some(Step::Checkpoint(next)))
                        }
                        Step::Published { .. } => Err(error(
                            FailureKind::Uncertain,
                            "Unexpected Bluesky video reconciliation result",
                        )),
                    };
                }
                if matches!(
                    checkpoint_stage(cp),
                    "bsky_video_upload_started" | "bsky_image_upload_started"
                ) {
                    return Ok(None);
                }
                let expected = super::bluesky::bluesky_record(p, cp)?;
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
                if v["value"] != expected {
                    return Err(error(
                        FailureKind::Uncertain,
                        "Existing Bluesky record does not match the publication snapshot",
                    ));
                }
                Ok(Some(super::bluesky::bluesky_receipt(c, cp, &v, true)?))
            }
            _ => Ok(None),
        }
    }
}
