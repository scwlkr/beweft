use super::*;
impl Adapter {
    pub async fn metrics(&self, c: &Channel, external_id: &str) -> Result<Vec<Metric>> {
        require_permission(c, "metrics")?;
        let token = Some(c.tokens.access_token.as_str());
        let id = segment(external_id);
        match c.app.provider.as_str() {
            "x" => {
                let v = self
                    .send(
                        self.request(
                            Method::GET,
                            &format!("https://api.x.com/2/tweets/{id}?tweet.fields=public_metrics"),
                            token,
                        )?,
                        false,
                    )
                    .await?;
                Ok(metrics(
                    &v["data"]["public_metrics"],
                    &[
                        "retweet_count",
                        "reply_count",
                        "like_count",
                        "quote_count",
                        "bookmark_count",
                        "impression_count",
                    ],
                ))
            }
            "youtube" => {
                let v = self
                    .send(
                        self.request(
                            Method::GET,
                            "https://www.googleapis.com/youtube/v3/videos",
                            token,
                        )?
                        .query(&[("part", "statistics"), ("id", external_id)]),
                        false,
                    )
                    .await?;
                Ok(metrics(
                    &v["items"][0]["statistics"],
                    &["viewCount", "likeCount", "commentCount"],
                ))
            }
            "facebook" => {
                let v=self.send(self.request(Method::GET,&format!("https://graph.facebook.com/v23.0/{id}?fields=reactions.limit(0).summary(true),comments.limit(0).summary(true)"),token)?,false).await?;
                Ok(metrics(
                    &json!({"reactions":v["reactions"]["summary"]["total_count"],"comments":v["comments"]["summary"]["total_count"]}),
                    &["reactions", "comments"],
                ))
            }
            "instagram" => {
                let v=self.send(self.request(Method::GET,&format!("https://graph.instagram.com/v23.0/{id}?fields=like_count,comments_count"),token)?,false).await?;
                Ok(metrics(&v, &["like_count", "comments_count"]))
            }
            "threads" => {
                let v=self.send(self.request(Method::GET,&format!("https://graph.threads.net/v1.0/{id}/insights?metric=views,likes,replies,reposts,quotes"),token)?,false).await?;
                Ok(v["data"]
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .filter_map(|m| {
                        Some(Metric {
                            name: m["name"].as_str()?.into(),
                            value: m["values"][0]["value"].as_i64()?,
                        })
                    })
                    .collect())
            }
            "mastodon" => {
                let v = self
                    .send(
                        self.request(
                            Method::GET,
                            &format!("{}/api/v1/statuses/{id}", instance_origin(&c.app)?),
                            token,
                        )?,
                        false,
                    )
                    .await?;
                Ok(metrics(
                    &v,
                    &["reblogs_count", "favourites_count", "replies_count"],
                ))
            }
            "bluesky" => {
                let v = self
                    .send(
                        self.request(
                            Method::GET,
                            "https://public.api.bsky.app/xrpc/app.bsky.feed.getPosts",
                            None,
                        )?
                        .query(&[("uris", external_id)]),
                        false,
                    )
                    .await?;
                Ok(metrics(
                    &v["posts"][0],
                    &["replyCount", "repostCount", "likeCount", "quoteCount"],
                ))
            }
            _ => Err(error(
                FailureKind::Unavailable,
                "Metrics are not implemented for this provider",
            )),
        }
    }
}
