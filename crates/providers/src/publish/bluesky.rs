use super::*;
impl Adapter {
    pub(super) async fn publish_bluesky(
        &self,
        c: &Channel,
        p: &Publication,
        cp: &Value,
    ) -> Result<Step> {
        let token = Some(c.tokens.access_token.as_str());
        let stage = checkpoint_stage(cp);
        let origin = instance_origin(&c.app)?;
        if stage.is_empty() {
            let micros = chrono::Utc::now().timestamp_micros() as u64;
            let mut n = micros << 10;
            let alphabet = b"234567abcdefghijklmnopqrstuvwxyz";
            let mut chars = [b'2'; 13];
            for i in (0..13).rev() {
                chars[i] = alphabet[(n & 31) as usize];
                n >>= 5;
            }
            return Ok(Step::Checkpoint(
                json!({"stage":"prepared","rkey":String::from_utf8(chars.to_vec()).unwrap(),"created_at":chrono::Utc::now().to_rfc3339(),"blobs":[]}),
            ));
        }
        let mut blobs = cp["blobs"].as_array().cloned().unwrap_or_default();
        if blobs.len() < p.media.len() {
            let m = &p.media[blobs.len()];
            let v = self
                .send(
                    self.request(
                        Method::POST,
                        &format!("{origin}/xrpc/com.atproto.repo.uploadBlob"),
                        token,
                    )?
                    .header("Content-Type", &m.mime)
                    .body(m.source.body(0, m.source.len()).await?),
                    true,
                )
                .await?;
            if !v["blob"].is_object() {
                return Err(error(
                    FailureKind::Uncertain,
                    "Bluesky upload returned no blob",
                ));
            }
            blobs.push(v["blob"].clone());
            let mut next = cp.clone();
            next["blobs"] = json!(blobs);
            return Ok(Step::Checkpoint(next));
        }
        let mut record =
            json!({"$type":"app.bsky.feed.post","text":p.text,"createdAt":cp["created_at"]});
        if !blobs.is_empty() {
            record["embed"] = json!({"$type":"app.bsky.embed.images","images":blobs.iter().enumerate().map(|(i,b)|json!({"image":b,"alt":p.media[i].alt_text})).collect::<Vec<_>>()});
        }
        let v=self.send(self.request(Method::POST,&format!("{origin}/xrpc/com.atproto.repo.createRecord"),token)?.json(&json!({"repo":c.identity.id,"collection":"app.bsky.feed.post","rkey":cp["rkey"],"record":record})),true).await?;
        let uri = field(&v, "uri")?;
        let rkey = field(cp, "rkey")?;
        Ok(published(
            uri.clone(),
            Some(format!(
                "https://bsky.app/profile/{}/post/{rkey}",
                c.identity.id
            )),
            json!({"uri":uri,"cid":v["cid"]}),
        ))
    }
}
