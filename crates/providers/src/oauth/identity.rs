use super::*;
impl Adapter {
    pub async fn identities(&self, app: &AppConfig, tokens: &Tokens) -> Result<Vec<Channel>> {
        let token = Some(tokens.access_token.as_str());
        let mut result = vec![];
        if app.provider == "facebook" {
            let v = self
                .send(
                    self.request(
                        Method::GET,
                        "https://graph.facebook.com/v23.0/me/accounts",
                        token,
                    )?
                    .query(&[("fields", "id,name,access_token,tasks"), ("limit", "100")]),
                    false,
                )
                .await?;
            for item in v["data"]
                .as_array()
                .ok_or_else(|| invalid("Provider returned no Page list"))?
            {
                if !item["tasks"].as_array().is_some_and(|a| {
                    a.iter().any(|x| {
                        x == "CREATE_CONTENT"
                            || x == "MANAGE"
                            || x == "PROFILE_PLUS_CREATE_CONTENT"
                            || x == "PROFILE_PLUS_FULL_CONTROL"
                    })
                }) {
                    continue;
                }
                let mut page_tokens = tokens.clone();
                page_tokens.access_token = field(item, "access_token")?;
                result.push(Channel {
                    app: app.clone(),
                    tokens: page_tokens,
                    identity: Identity {
                        id: field(item, "id")?,
                        name: field(item, "name")?,
                        username: None,
                    },
                });
            }
            return Ok(result);
        }
        let url = match app.provider.as_str() {
            "x" => "https://api.x.com/2/users/me".into(),
            "youtube" => {
                "https://www.googleapis.com/youtube/v3/channels?part=snippet&mine=true".into()
            }
            "instagram" => {
                "https://graph.instagram.com/v23.0/me?fields=user_id,username,name".into()
            }
            "threads" => "https://graph.threads.net/v1.0/me?fields=id,username,name".into(),
            "mastodon" => format!(
                "{}/api/v1/accounts/verify_credentials",
                instance_origin(app)?
            ),
            "linkedin" => "https://api.linkedin.com/v2/userinfo".into(),
            "pinterest" => "https://api.pinterest.com/v5/user_account".into(),
            _ => return Err(invalid("Unknown identity provider")),
        };
        let v = self
            .send(self.request(Method::GET, &url, token)?, false)
            .await?;
        let item = match app.provider.as_str() {
            "x" => &v["data"],
            "youtube" => &v["items"][0],
            _ => &v,
        };
        let id = match app.provider.as_str() {
            "instagram" => field(item, "user_id")?,
            "linkedin" => field(item, "sub")?,
            "pinterest" => field(item, "username")?,
            _ => field(item, "id")?,
        };
        let name = if app.provider == "youtube" {
            item["snippet"]["title"].as_str()
        } else {
            item["name"]
                .as_str()
                .or_else(|| item["display_name"].as_str())
                .or_else(|| item["username"].as_str())
        }
        .unwrap_or(&id)
        .to_owned();
        result.push(Channel {
            app: app.clone(),
            tokens: tokens.clone(),
            identity: Identity {
                id,
                name,
                username: item["username"].as_str().map(str::to_owned),
            },
        });
        Ok(result)
    }
    pub async fn connect_bluesky(
        &self,
        app: &AppConfig,
        identifier: &str,
        password: &str,
    ) -> Result<Channel> {
        let origin = instance_origin(app)?;
        let v = self
            .send(
                self.request(
                    Method::POST,
                    &format!("{origin}/xrpc/com.atproto.server.createSession"),
                    None,
                )?
                .json(&json!({"identifier":identifier,"password":password})),
                false,
            )
            .await?;
        if v["active"].as_bool() == Some(false) {
            return Err(invalid("Bluesky account is not active"));
        }
        let identity = Identity {
            id: field(&v, "did")?,
            name: field(&v, "handle")?,
            username: Some(field(&v, "handle")?),
        };
        Ok(Channel {
            app: app.clone(),
            identity,
            tokens: Tokens {
                access_token: field(&v, "accessJwt")?,
                refresh_token: Some(field(&v, "refreshJwt")?),
                expires_at: Some(chrono::Utc::now().timestamp() + 3600),
                scopes: vec![],
                scopes_known: false,
            },
        })
    }
    /// Returns false when the network does not expose implemented revocation.
    pub async fn revoke(&self, channel: &Channel) -> Result<bool> {
        let app = &channel.app;
        let access = &channel.tokens.access_token;
        let request = match app.provider.as_str() {
            "x" => self
                .request(Method::POST, "https://api.x.com/2/oauth2/revoke", None)?
                .basic_auth(&app.client_id, Some(&app.client_secret))
                .form(&[("token", access.as_str()), ("client_id", &app.client_id)]),
            "youtube" => self
                .request(Method::POST, "https://oauth2.googleapis.com/revoke", None)?
                .form(&[("token", access.as_str())]),
            "facebook" => self.request(
                Method::DELETE,
                "https://graph.facebook.com/v23.0/me/permissions",
                Some(access),
            )?,
            "mastodon" => self
                .request(
                    Method::POST,
                    &format!("{}/oauth/revoke", instance_origin(app)?),
                    None,
                )?
                .form(&[
                    ("client_id", app.client_id.as_str()),
                    ("client_secret", app.client_secret.as_str()),
                    ("token", access.as_str()),
                ]),
            "bluesky" => self.request(
                Method::POST,
                &format!(
                    "{}/xrpc/com.atproto.server.deleteSession",
                    instance_origin(app)?
                ),
                channel.tokens.refresh_token.as_deref(),
            )?,
            _ => return Ok(false),
        };
        self.response(request, false).await?;
        Ok(true)
    }
}
