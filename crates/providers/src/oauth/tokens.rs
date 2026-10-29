use super::*;
impl Adapter {
    pub async fn exchange(
        &self,
        app: &AppConfig,
        code: &str,
        redirect_uri: &str,
        verifier: &str,
    ) -> Result<Tokens> {
        let mut form = vec![
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", &app.client_id),
        ];
        if matches!(app.provider.as_str(), "x" | "youtube" | "mastodon") {
            form.push(("code_verifier", verifier));
        }
        let mut request = self.request(Method::POST, &token_endpoint(app)?, None)?;
        if matches!(app.provider.as_str(), "x" | "pinterest") {
            request = request.basic_auth(&app.client_id, Some(&app.client_secret));
        } else {
            form.push(("client_secret", &app.client_secret));
        }
        let v = self.send(request.form(&form), false).await?;
        let mut tokens = parse_tokens(&v, None)?;
        if matches!(app.provider.as_str(), "facebook" | "instagram" | "threads") {
            let (url, grant) = match app.provider.as_str() {
                "facebook" => (
                    "https://graph.facebook.com/v23.0/oauth/access_token",
                    "fb_exchange_token",
                ),
                "instagram" => (
                    "https://graph.instagram.com/access_token",
                    "ig_exchange_token",
                ),
                _ => (
                    "https://graph.threads.net/access_token",
                    "th_exchange_token",
                ),
            };
            let mut q = vec![
                ("grant_type", grant),
                ("client_secret", app.client_secret.as_str()),
            ];
            if app.provider == "facebook" {
                q.push(("client_id", &app.client_id));
                q.push(("fb_exchange_token", &tokens.access_token));
            } else {
                q.push(("access_token", &tokens.access_token));
            }
            tokens = parse_tokens(
                &self
                    .send(self.request(Method::GET, url, None)?.query(&q), false)
                    .await?,
                Some(&tokens),
            )?;
        }
        // The provider may omit scope on exchange. These are requested scopes,
        // not asserted granted scopes. Keep unknown grants empty.
        Ok(tokens)
    }
    pub async fn refresh(&self, app: &AppConfig, tokens: &Tokens) -> Result<Tokens> {
        if app.provider == "bluesky" {
            let origin = app
                .instance_url
                .clone()
                .unwrap_or_else(|| "https://bsky.social".into());
            let v = self
                .send(
                    self.request(
                        Method::POST,
                        &format!("{origin}/xrpc/com.atproto.server.refreshSession"),
                        tokens.refresh_token.as_deref(),
                    )?,
                    false,
                )
                .await?;
            return Ok(Tokens {
                access_token: field(&v, "accessJwt")?,
                refresh_token: Some(field(&v, "refreshJwt")?),
                expires_at: Some(chrono::Utc::now().timestamp() + 3600),
                scopes: tokens.scopes.clone(),
                scopes_known: tokens.scopes_known,
            });
        }
        if matches!(app.provider.as_str(), "threads" | "instagram") {
            let (host, grant) = if app.provider == "threads" {
                (
                    "https://graph.threads.net/refresh_access_token",
                    "th_refresh_token",
                )
            } else {
                (
                    "https://graph.instagram.com/refresh_access_token",
                    "ig_refresh_token",
                )
            };
            return parse_tokens(
                &self
                    .send(
                        self.request(Method::GET, host, None)?.query(&[
                            ("grant_type", grant),
                            ("access_token", tokens.access_token.as_str()),
                        ]),
                        false,
                    )
                    .await?,
                Some(tokens),
            );
        }
        let refresh = tokens.refresh_token.as_deref().ok_or_else(|| {
            error(
                FailureKind::Authentication,
                "Provider has no renewable grant. Reconnect this channel.",
            )
        })?;
        let mut form = vec![
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh),
            ("client_id", app.client_id.as_str()),
        ];
        let mut request = self.request(Method::POST, &token_endpoint(app)?, None)?;
        if matches!(app.provider.as_str(), "x" | "pinterest") {
            request = request.basic_auth(&app.client_id, Some(&app.client_secret));
        } else {
            form.push(("client_secret", &app.client_secret));
        }
        parse_tokens(&self.send(request.form(&form), false).await?, Some(tokens))
    }
}
