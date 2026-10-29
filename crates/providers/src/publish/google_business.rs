use super::*;
use chrono::{Datelike, Timelike};

// An HTTPS label alone is not a remotely fetchable origin. Reject obvious
// local and reserved names/IPs; the operator still has to qualify real DNS,
// TLS and public reachability for their configured origin.
pub fn google_public_origin(origin: &str) -> bool {
    let Ok(url) = url::Url::parse(origin) else {
        return false;
    };
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return false;
    }
    match url.host() {
        Some(url::Host::Domain(name)) => {
            let name = name.trim_end_matches('.').to_ascii_lowercase();
            name.contains('.')
                && !["example.com", "example.net", "example.org"]
                    .iter()
                    .any(|suffix| name == *suffix || name.ends_with(&format!(".{suffix}")))
                && ![
                    "localhost",
                    "local",
                    "invalid",
                    "example",
                    "test",
                    "fixture",
                    "internal",
                    "lan",
                    "home.arpa",
                ]
                .iter()
                .any(|suffix| name == *suffix || name.ends_with(&format!(".{suffix}")))
        }
        Some(url::Host::Ipv4(ip)) => {
            let [a, b, c, _] = ip.octets();
            !ip.is_private()
                && !ip.is_loopback()
                && !ip.is_link_local()
                && !ip.is_multicast()
                && !ip.is_unspecified()
                && a != 0
                && a < 224
                && !(a == 100 && (64..=127).contains(&b))
                && !(a == 192 && b == 0 && (c == 0 || c == 2))
                && !(a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100)))
                && !(a == 203 && b == 0 && c == 113)
        }
        Some(url::Host::Ipv6(ip)) => {
            !ip.is_loopback()
                && !ip.is_unspecified()
                && !ip.is_multicast()
                && !ip.is_unique_local()
                && !ip.is_unicast_link_local()
                && ip.to_ipv4_mapped().is_none()
                && !(ip.segments()[0] == 0x2001 && ip.segments()[1] == 0x0db8)
        }
        None => false,
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    #[serde(default = "standard")]
    topic_type: String,
    language_code: Option<String>,
    event: Option<Event>,
    call_to_action: Option<CallToAction>,
    offer: Option<Offer>,
}
fn standard() -> String {
    "STANDARD".into()
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Event {
    title: String,
    start_date: String,
    start_time: String,
    end_date: String,
    end_time: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CallToAction {
    #[serde(rename = "type")]
    kind: String,
    url: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Offer {
    coupon_code: Option<String>,
    redeem_online_url: Option<String>,
    terms_conditions: Option<String>,
}

fn bounded(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.chars().count() <= max
}
fn link(value: &str) -> bool {
    value.len() <= 2048
        && Url::parse(value).is_ok_and(|u| {
            matches!(u.scheme(), "http" | "https")
                && u.host_str().is_some()
                && u.username().is_empty()
                && u.password().is_none()
        })
}
fn local_time(date: &str, time: &str) -> Result<chrono::NaiveDateTime> {
    if date.len() != 10 || time.len() != 5 {
        return Err(invalid(
            "Google event dates use YYYY-MM-DD and times use HH:MM in the location's local time",
        ));
    }
    let date = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|_| invalid("Invalid Google event date"))?;
    if !(1..=9999).contains(&date.year()) {
        return Err(invalid("Invalid Google event year"));
    }
    let time = chrono::NaiveTime::parse_from_str(time, "%H:%M")
        .map_err(|_| invalid("Invalid Google event time"))?;
    Ok(date.and_time(time))
}
fn date(value: chrono::NaiveDateTime) -> Value {
    json!({"year":value.year(),"month":value.month(),"day":value.day()})
}
fn time(value: chrono::NaiveDateTime) -> Value {
    json!({"hours":value.hour(),"minutes":value.minute(),"seconds":0,"nanos":0})
}

/// Validate the complete options object before scheduling and build Google's
/// documented LocalPost body. Media URLs are minted only during delivery.
pub(super) fn post_body(p: &Publication, delivery: bool) -> Result<Value> {
    let options: Options = serde_json::from_value(p.options.clone())
        .map_err(|_| invalid("Invalid or unsupported Google Business Profile options"))?;
    if !matches!(options.topic_type.as_str(), "STANDARD" | "EVENT" | "OFFER") {
        return Err(invalid(
            "Google Business Profile supports STANDARD, EVENT and OFFER posts; product posts are not supported",
        ));
    }
    if p.text.chars().count() > 1500 {
        return Err(invalid(
            "Google Business Profile supports up to 1,500 text characters",
        ));
    }
    if p.media.len() > 1
        || p.media.iter().any(|m| {
            !matches!(m.mime.as_str(), "image/jpeg" | "image/png") || !m.alt_text.trim().is_empty()
        })
    {
        return Err(invalid(
            "Google Business Profile supports one JPEG/PNG photo; photo descriptions and video are not supported by this adapter",
        ));
    }
    let mut body = json!({"topicType":options.topic_type,"summary":p.text});
    if let Some(language) = options.language_code {
        let mut parts = language.split('-');
        let valid = parts.next().is_some_and(|p| {
            (2..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphabetic())
        }) && parts
            .all(|p| (1..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphanumeric()));
        if language.len() > 35 || !valid {
            return Err(invalid("Invalid Google post language code"));
        }
        body["languageCode"] = json!(language);
    }
    match (options.topic_type.as_str(), options.event) {
        ("EVENT" | "OFFER", Some(event)) => {
            if !bounded(&event.title, 256) {
                return Err(invalid(
                    "Google event title is required and must be at most 256 characters",
                ));
            }
            let start = local_time(&event.start_date, &event.start_time)?;
            let end = local_time(&event.end_date, &event.end_time)?;
            if end <= start {
                return Err(invalid(
                    "Google event end must be after its start in the location's local time",
                ));
            }
            body["event"] = json!({"title":event.title,"schedule":{"startDate":date(start),"startTime":time(start),"endDate":date(end),"endTime":time(end)}});
        }
        ("EVENT" | "OFFER", None) => {
            return Err(invalid(
                "Google event and offer posts require an event title, dates and times",
            ));
        }
        (_, Some(_)) => return Err(invalid("Event details require an EVENT or OFFER post")),
        _ => {}
    }
    if let Some(cta) = options.call_to_action {
        if options.topic_type == "OFFER" {
            return Err(invalid(
                "Google ignores offer call-to-action buttons; remove the offer CTA",
            ));
        }
        if !matches!(
            cta.kind.as_str(),
            "BOOK" | "ORDER" | "SHOP" | "LEARN_MORE" | "SIGN_UP" | "CALL"
        ) {
            return Err(invalid("Unsupported Google call-to-action type"));
        }
        if cta.kind == "CALL" {
            if cta.url.is_some() {
                return Err(invalid("Google CALL actions must not include a URL"));
            }
            body["callToAction"] = json!({"actionType":cta.kind});
        } else {
            let url = cta.url.filter(|s| link(s)).ok_or_else(|| {
                invalid("Google call-to-action requires an HTTP(S) URL without credentials")
            })?;
            body["callToAction"] = json!({"actionType":cta.kind,"url":url});
        }
    }
    if let Some(offer) = options.offer {
        if options.topic_type != "OFFER" {
            return Err(invalid("Offer details require an OFFER post"));
        }
        let mut value = json!({});
        if let Some(code) = offer.coupon_code {
            if !bounded(&code, 256) {
                return Err(invalid("Invalid Google offer coupon code"));
            }
            value["couponCode"] = json!(code);
        }
        if let Some(url) = offer.redeem_online_url {
            if !link(&url) {
                return Err(invalid("Invalid Google offer redemption URL"));
            }
            value["redeemOnlineUrl"] = json!(url);
        }
        if let Some(terms) = offer.terms_conditions {
            if !bounded(&terms, 2048) {
                return Err(invalid("Invalid Google offer terms"));
            }
            value["termsConditions"] = json!(terms);
        }
        body["offer"] = value;
    }
    if delivery && let Some(media) = p.media.first() {
        let url = public_media(media)?;
        if !google_public_origin(url) {
            return Err(invalid(
                "Google requires a temporary public HTTPS photo URL",
            ));
        }
        body["media"] = json!([{"mediaFormat":"PHOTO","sourceUrl":url}]);
    }
    Ok(body)
}

fn post_name(cp: &Value, c: &Channel) -> Result<String> {
    let name = field(cp, "name")?;
    let (parent, post) = name
        .split_once("/localPosts/")
        .ok_or_else(|| error(FailureKind::Uncertain, "Invalid saved Google post resource"))?;
    let (account, location) = parent
        .split_once("/locations/")
        .ok_or_else(|| error(FailureKind::Uncertain, "Invalid saved Google post resource"))?;
    google_resource(account, "accounts")
        .map_err(|_| error(FailureKind::Uncertain, "Invalid saved Google account"))?;
    google_resource(&format!("locations/{location}"), "locations")
        .map_err(|_| error(FailureKind::Uncertain, "Invalid saved Google location"))?;
    google_resource(&format!("localPosts/{post}"), "localPosts").map_err(|_| {
        error(
            FailureKind::Uncertain,
            "Invalid saved Google post identifier",
        )
    })?;
    if c.identity.id != format!("locations/{location}") {
        return Err(error(
            FailureKind::Uncertain,
            "Saved Google post belongs to a different location",
        ));
    }
    Ok(name)
}

impl Adapter {
    pub(super) async fn publish_google_business(
        &self,
        c: &Channel,
        p: &Publication,
        cp: &Value,
    ) -> Result<Step> {
        let stage = checkpoint_stage(cp);
        if stage.is_empty() {
            // Check the current media capability before crossing the public
            // boundary. The operator may have changed the origin after queueing.
            post_body(p, true)?;
            return Ok(Step::Checkpoint(
                json!({"stage":"google_post_started","parent":c.selection_key()?}),
            ));
        }
        if stage == "google_post_started" {
            let parent = field(cp, "parent")?;
            if c.selection_key()? != parent {
                return Err(error(
                    FailureKind::Uncertain,
                    "Google account selection changed at the posting boundary; inspect the original outcome",
                ));
            }
            let body = post_body(p, true)?;
            let value = self.send(self.request(Method::POST, &format!("https://mybusiness.googleapis.com/v4/{parent}/localPosts"), Some(&c.tokens.access_token))?.json(&body), true).await
                .map_err(|_| error(FailureKind::Uncertain, "Google post creation outcome is unknown. Inspect the original attempt before resolving it."))?;
            let name = field(&value, "name")?;
            let next = json!({"stage":"google_post_processing","name":name});
            let actual = post_name(&next, c)?;
            if !actual.starts_with(&format!("{parent}/localPosts/")) {
                return Err(error(
                    FailureKind::Uncertain,
                    "Google returned a post for a different parent",
                ));
            }
            return Ok(Step::Checkpoint(next));
        }
        if stage == "google_post_processing" {
            return self.google_post_status(c, cp).await;
        }
        Err(error(
            FailureKind::Uncertain,
            "Google post has already crossed its creation boundary. Inspect its outcome before retrying.",
        ))
    }

    pub(super) async fn google_post_status(&self, c: &Channel, cp: &Value) -> Result<Step> {
        let name = post_name(cp, c)?;
        let value = self
            .send(
                self.request(
                    Method::GET,
                    &format!("https://mybusiness.googleapis.com/v4/{name}"),
                    Some(&c.tokens.access_token),
                )?,
                false,
            )
            .await?;
        if value["name"].as_str() != Some(name.as_str()) {
            return Err(error(
                FailureKind::Uncertain,
                "Google returned a different or missing post resource",
            ));
        }
        match value["state"].as_str() {
            Some("LIVE" | "RECURRING") => {
                let url = value["searchUrl"]
                    .as_str()
                    .filter(|s| link(s))
                    .map(str::to_owned);
                Ok(published(
                    name.clone(),
                    url,
                    json!({"name":name,"state":value["state"],"verification":"provider"}),
                ))
            }
            Some("REJECTED") => {
                let mut next = cp.clone();
                next["stage"] = json!("google_post_rejected");
                Ok(Step::Checkpoint(next))
            }
            _ => Ok(Step::Wait(cp.clone())),
        }
    }
}
