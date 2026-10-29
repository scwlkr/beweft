use super::*;
use std::{collections::HashSet, time::Duration};

/// One bounded local inspection budget per atomic scheduling mutation. Media
/// bytes shared by channels of the same provider are inspected only once.
pub struct ScheduleFormatValidator {
    deadline: tokio::time::Instant,
    inspected: HashSet<(String, Vec<Uuid>)>,
}

impl Default for ScheduleFormatValidator {
    fn default() -> Self {
        Self {
            deadline: tokio::time::Instant::now() + Duration::from_secs(12),
            inspected: HashSet::new(),
        }
    }
}

impl ScheduleFormatValidator {
    pub async fn validate(
        &mut self,
        state: &AppState,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        channel: Uuid,
        brand: Uuid,
        payload: &Value,
    ) -> Result<()> {
        let provider: String =
            sqlx::query_scalar("SELECT provider FROM channels WHERE id=$1 AND brand_id=$2")
                .bind(channel)
                .bind(brand)
                .fetch_one(&mut **tx)
                .await?;
        if !providers::known(&provider) {
            // Manual, fixture and legacy unknown channels keep their existing behavior.
            if payload["text"].as_str().unwrap_or("").trim().is_empty()
                && payload["media_ids"].as_array().is_none_or(Vec::is_empty)
            {
                return Err(ApiError::bad("A scheduled post needs text or media"));
            }
            return Ok(());
        }
        let value = json!({"id":Uuid::nil(),"brand_id":brand,"payload":payload});
        let p = preparation::prepare_publication_on(state, tx, &value, false).await?;
        providers::validate(&provider, &p).map_err(format_error)?;
        if provider == "google_business"
            && !p.media.is_empty()
            && !providers::google_public_origin(&state.config.public_url)
        {
            return Err(ApiError::bad_code(
                "provider_format_invalid",
                "Google photos require an actual public HTTPS server origin; local, private and reserved origins cannot be fetched.",
            ));
        }
        if !p.media.is_empty()
            && matches!(
                provider.as_str(),
                "facebook" | "instagram" | "threads" | "pinterest" | "google_business"
            )
            && !(provider == "facebook" && p.options["format"] == "reel")
            && !state.config.public_url.starts_with("https://")
        {
            return Err(ApiError::bad_code(
                "provider_format_invalid",
                "This media format needs a public HTTPS server origin for provider delivery.",
            ));
        }
        let ids: Vec<Uuid> =
            serde_json::from_value(payload["media_ids"].clone()).map_err(ApiError::internal)?;
        let key = (provider.clone(), ids);
        if !p.media.is_empty() && !self.inspected.contains(&key) {
            tokio::time::timeout_at(self.deadline, providers::inspect_media(&provider, &p))
                .await
                .map_err(|_| unavailable("Local media inspection exceeded its time limit."))?
                .map_err(format_error)?;
            self.inspected.insert(key);
        }
        Ok(())
    }
}

fn unavailable(message: &str) -> ApiError {
    ApiError(
        StatusCode::SERVICE_UNAVAILABLE,
        "provider_format_check_unavailable",
        message.into(),
    )
}

fn format_error(error: providers::ProviderError) -> ApiError {
    if error.kind == FailureKind::Unavailable {
        unavailable(&error.message)
    } else {
        ApiError::bad_code("provider_format_invalid", error.message)
    }
}
