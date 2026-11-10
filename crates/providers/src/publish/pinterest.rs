use super::*;
impl Adapter {
    pub(super) async fn publish_pinterest(
        &self,
        c: &Channel,
        p: &Publication,
        cp: &Value,
    ) -> Result<Step> {
        let token = Some(c.tokens.access_token.as_str());
        let videos = p.media.iter().filter(|m| video(m)).count();
        if videos == 0 {
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
                        token,
                    )?
                    .json(&body),
                    true,
                )
                .await?;
            let id = field(&v, "id")?;
            return Ok(published(
                id.clone(),
                Some(format!("https://www.pinterest.com/pin/{id}/")),
                json!({"id":id}),
            ));
        }

        // Video Pin via Pinterest media upload registration.
        if cp["expired"] == true {
            return Err(error(
                FailureKind::Uncertain,
                "Pinterest upload session expired. Reconcile the outcome and explicitly reset before starting a new upload.",
            ));
        }
        let m = &p.media[0];
        let cover = p
            .options
            .get("cover_image_url")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let stage = checkpoint_stage(cp);
        if stage.is_empty() {
            let v = self
                .send(
                    self.request(
                        Method::POST,
                        "https://api.pinterest.com/v5/media",
                        token,
                    )?
                    .json(&json!({"media_type": "video"})),
                    true,
                )
                .await?;
            let media_id = field(&v, "media_id")?;
            let upload_url = v["upload_url"]
                .as_str()
                .ok_or_else(|| {
                    error(
                        FailureKind::Uncertain,
                        "Pinterest did not return an upload URL; reconcile before retrying",
                    )
                })?
                .to_owned();
            if self.fixture_origin.is_none() {
                let u = Url::parse(&upload_url).map_err(|_| invalid("Invalid Pinterest upload URL"))?;
                if u.scheme() != "https"
                    || u.host_str() != Some("pinterest-media-upload.s3-accelerate.amazonaws.com")
                {
                    return Err(invalid("Unexpected Pinterest upload host"));
                }
            }
            let upload_parameters = v
                .get("upload_parameters")
                .cloned()
                .unwrap_or_else(|| json!({}));
            return Ok(Step::Checkpoint(json!({
                "stage": "pinterest_media_registered",
                "media_id": media_id,
                "upload_url": upload_url,
                "upload_parameters": upload_parameters,
                "size": m.source.len(),
            })));
        }

        let media_id = field(cp, "media_id")?;
        let upload_url = cp["upload_url"]
            .as_str()
            .ok_or_else(|| invalid("Pinterest checkpoint is missing an upload URL"))?
            .to_owned();
        let upload_parameters = cp["upload_parameters"].clone();
        let size = cp["size"].as_u64().unwrap_or(m.source.len());

        if stage == "pinterest_media_registered" {
            let mut form = Form::new();
            if let Some(params) = upload_parameters.as_object() {
                for (key, value) in params {
                    if let Some(text) = value.as_str() {
                        form = form.text(key.clone(), text.to_owned());
                    } else {
                        form = form.text(key.clone(), value.to_string());
                    }
                }
            }
            form = form.part(
                "file".to_owned(),
                Part::stream_with_length(m.source.body(0, size).await?, size)
                    .file_name(m.name.clone())
                    .mime_str(&m.mime)
                    .map_err(|_| invalid("Invalid media content type"))?,
            );
            let request = self
                .request(Method::POST, &upload_url, None)?
                .multipart(form);
            let response = self.raw_response(request, true).await?;
            if !response.status().is_success() {
                return Err(error(
                    if response.status().is_server_error() {
                        FailureKind::Uncertain
                    } else {
                        FailureKind::Rejected
                    },
                    "Pinterest rejected the media upload; inspect the saved registration before retrying",
                ));
            }
            // Drain the response body so the connection can be reused.
            let _ = response.bytes().await;
            let mut next = cp.clone();
            next["stage"] = json!("pinterest_media_uploaded");
            return Ok(Step::Checkpoint(next));
        }

        if stage == "pinterest_media_uploaded" {
            let v = self
                .send(
                    self.request(
                        Method::GET,
                        &format!("https://api.pinterest.com/v5/media/{}", id_path(&media_id)),
                        token,
                    )?,
                    false,
                )
                .await?;
            let status = v["status"].as_str().unwrap_or("");
            if status == "failed" {
                return Err(error(
                    FailureKind::Rejected,
                    "Pinterest reported that the media upload failed",
                ));
            }
            if status != "succeeded" {
                let next = cp.clone();
                return Ok(Step::Wait(next));
            }
            let body = json!({
                "board_id": p.options["board_id"],
                "title": p.title,
                "description": p.text,
                "link": p.options["link"],
                "media_source": {
                    "source_type": "video_id",
                    "cover_image_url": cover,
                    "media_id": media_id
                }
            });
            let v = self
                .send(
                    self.request(
                        Method::POST,
                        "https://api.pinterest.com/v5/pins",
                        token,
                    )?
                    .json(&body),
                    true,
                )
                .await?;
            let id = field(&v, "id")?;
            return Ok(published(
                id.clone(),
                Some(format!("https://www.pinterest.com/pin/{id}/")),
                json!({"id":id}),
            ));
        }

        Err(invalid("Unknown Pinterest upload checkpoint"))
    }
}
