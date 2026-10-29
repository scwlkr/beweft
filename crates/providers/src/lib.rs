//! Direct network adapters. No credentials are logged or included in errors.
//! Each publishing step returns a checkpoint which MUST be durably saved before
//! the next step. A lost response is uncertain, never permission to resubmit.
mod oauth;
mod publish;
mod read;
pub use oauth::*;
pub use publish::*;
pub use read::*;
use reqwest::{Client, Method, RequestBuilder};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;
use url::Url;

mod catalog;
mod client;
mod types;
pub use catalog::*;
pub use client::*;
pub use types::*;
pub fn instance_origin(app: &AppConfig) -> Result<String> {
    let value = app
        .instance_url
        .as_deref()
        .ok_or_else(|| invalid("A server-configured instance URL is required"))?;
    let u = Url::parse(value).map_err(|_| invalid("Invalid instance origin"))?;
    if u.scheme() != "https"
        || u.path() != "/"
        || u.query().is_some()
        || u.fragment().is_some()
        || !u.username().is_empty()
        || u.password().is_some()
        || u.port().is_some()
    {
        return Err(invalid(
            "Instance URL must be a public HTTPS origin on the default port",
        ));
    }
    let h = u.host_str().unwrap_or_default();
    if h == "localhost"
        || h.ends_with(".localhost")
        || h.ends_with(".local")
        || h.parse::<std::net::IpAddr>().is_ok()
        || !h.contains('.')
    {
        return Err(invalid("Instance origin must be a public DNS hostname"));
    }
    Ok(u.origin().ascii_serialization())
}
pub(crate) fn field(v: &Value, key: &str) -> Result<String> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            error(
                FailureKind::Uncertain,
                "Provider response omitted a required identifier; reconcile before retrying",
            )
        })
}
pub fn checkpoint_stage(v: &Value) -> &str {
    v["stage"].as_str().unwrap_or("")
}
pub fn provider_metadata() -> Value {
    json!({"verification":"unverified_live","adapter_test":"fixtures_only"})
}

mod permissions;
pub use permissions::*;
