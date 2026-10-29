use super::*;
impl Adapter {
    pub(super) async fn publish_youtube(
        &self,
        c: &Channel,
        p: &Publication,
        cp: &Value,
    ) -> Result<Step> {
        if cp["expired"] == true {
            return Err(error(
                FailureKind::Uncertain,
                "YouTube session expired. Reconcile the outcome and explicitly reset before starting a new upload.",
            ));
        }
        let token = Some(c.tokens.access_token.as_str());
        let stage = checkpoint_stage(cp);
        if stage.is_empty() {
            let status = json!({"privacyStatus":p.options["privacy"],"selfDeclaredMadeForKids":p.options["made_for_kids"],"containsSyntheticMedia":p.options["contains_synthetic_media"].as_bool().unwrap_or(false)});
            let body = json!({"snippet":{"title":p.title,"description":p.text,"categoryId":p.options["category_id"].as_str().unwrap_or("22")},"status":status});
            let response=self.response(self.request(Method::POST,"https://www.googleapis.com/upload/youtube/v3/videos?uploadType=resumable&part=snippet,status",token)?.header("X-Upload-Content-Type",&p.media[0].mime).header("X-Upload-Content-Length",p.media[0].source.len()).json(&body),true).await?;
            let session = response
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| {
                    error(
                        FailureKind::Uncertain,
                        "YouTube did not return an upload session; reconcile before retrying",
                    )
                })?;
            if self.fixture_origin.is_none() {
                let u =
                    Url::parse(session).map_err(|_| invalid("Invalid YouTube upload session"))?;
                if u.host_str() != Some("www.googleapis.com")
                    || u.scheme() != "https"
                    || !u.path().starts_with("/upload/youtube/")
                {
                    return Err(invalid("Unexpected YouTube upload session host"));
                }
            }
            return Ok(Step::Checkpoint(
                json!({"stage":"youtube_upload","session":session,"size":p.media[0].source.len(),"offset":0}),
            ));
        }
        let session = field(cp, "session")?;
        let m = &p.media[0];
        let offset = cp["offset"].as_u64().unwrap_or(0);
        if offset >= m.source.len() {
            return Err(error(
                FailureKind::Uncertain,
                "YouTube upload needs a completion status probe",
            ));
        }
        let length = (m.source.len() - offset).min(8 * 1024 * 1024);
        let request = self
            .request(Method::PUT, &session, token)?
            .header("Content-Type", &m.mime)
            .header("Content-Length", length)
            .header(
                "Content-Range",
                format!(
                    "bytes {}-{}/{}",
                    offset,
                    offset + length - 1,
                    m.source.len()
                ),
            )
            .body(m.source.body(offset, length).await?);
        let response = self.raw_response(request, true).await?;
        if response.status().as_u16() == 308 {
            let next = received_offset(&response)?;
            if next <= offset || next > offset + length {
                return Err(error(
                    FailureKind::Uncertain,
                    "YouTube reported an unexpected upload offset",
                ));
            }
            let mut cp = cp.clone();
            cp["offset"] = json!(next);
            return Ok(Step::Checkpoint(cp));
        }
        if !response.status().is_success() {
            return Err(error(
                if response.status().is_server_error() || response.status().as_u16() == 404 {
                    FailureKind::Uncertain
                } else {
                    FailureKind::Rejected
                },
                "YouTube rejected this upload chunk; inspect its saved session before restarting",
            ));
        }
        let v = self.response_json(response, true).await?;
        let id = field(&v, "id")?;
        Ok(published(
            id.clone(),
            Some(format!("https://www.youtube.com/watch?v={id}")),
            json!({"id":id,"privacy":v["status"]["privacyStatus"],"upload_status":v["status"]["uploadStatus"]}),
        ))
    }
}
