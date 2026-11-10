use super::*;
use chrono::Datelike;
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
            "google_business" => {
                crate::google_post_name_for_location(external_id, &c.identity.id)?;
                let parent = c.selection_key()?;
                let end = chrono::Utc::now().date_naive();
                let start = end - chrono::Duration::days(30);
                let body = json!({
                    "localPostNames": [external_id],
                    "basicRequest": {
                        "metricRequests": [
                            {"metric":"LOCAL_POST_VIEWS_SEARCH","options":["AGGREGATED_TOTAL"]},
                            {"metric":"LOCAL_POST_ACTIONS_CALL_TO_ACTION","options":["AGGREGATED_TOTAL"]}
                        ],
                        "timeRange": {
                            "startDate": {"year":start.year(),"month":start.month(),"day":start.day()},
                            "endDate": {"year":end.year(),"month":end.month(),"day":end.day()}
                        }
                    }
                });
                let v = self
                    .send(
                        self.request(
                            Method::POST,
                            &format!("https://mybusiness.googleapis.com/v4/{parent}/localPosts:reportInsights"),
                            token,
                        )?
                        .json(&body),
                        false,
                    )
                    .await?;
                let mut out = vec![];
                for m in v["localPostMetrics"].as_array().unwrap_or(&vec![]) {
                    if m["localPostName"].as_str() != Some(external_id) {
                        continue;
                    }
                    for mv in m["metricValues"].as_array().unwrap_or(&vec![]) {
                        let name = mv["metric"].as_str().unwrap_or("").to_owned();
                        if name.is_empty() {
                            continue;
                        }
                        let value = mv["totalValue"]["value"]
                            .as_str()
                            .and_then(|s| s.parse::<i64>().ok())
                            .or_else(|| mv["totalValue"]["value"].as_i64());
                        if let Some(value) = value {
                            out.push(Metric { name, value });
                        }
                        // Missing values are intentionally omitted rather than recorded as zero.
                    }
                }
                Ok(out)
            }
            _ => Err(error(
                FailureKind::Unavailable,
                "Metrics are not implemented for this provider",
            )),
        }
    }
}
