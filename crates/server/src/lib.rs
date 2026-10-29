pub mod account;
pub mod auth;
pub mod brand_admin;
pub mod catalog;
pub mod connections;
pub mod contract_export;
pub mod contracts;
pub mod crypto;
pub mod error;
pub mod health;
pub mod idea_groups;
pub mod ideas;
mod listing;
pub mod manual_channels;
pub mod media;
pub mod media_storage;
pub mod openapi;
mod openapi_operations;
pub mod posts;
pub mod provider_bridge;
pub mod publications;
pub mod queue_actions;
pub mod schedules;
pub mod snapshot;
pub mod state;
pub mod templates;
pub mod tokens;
pub mod worker;
use axum::{
    Router,
    extract::{DefaultBodyLimit, Request},
    http::header,
    middleware::{self, Next},
    response::Response,
    routing::{delete, get, patch, post},
};
use state::AppState;
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
pub fn router(state: AppState) -> Router {
    let upload_limit = state
        .config
        .max_upload_bytes
        .saturating_add(1024 * 1024)
        .min(usize::MAX as u64) as usize;
    let api = Router::new()
        .route("/auth/status", get(auth::status))
        .route("/auth/bootstrap", post(auth::bootstrap))
        .route("/auth/login", post(auth::login))
        .route("/auth/me", get(auth::me))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/password", post(account::password))
        .route("/auth/sessions", get(account::sessions))
        .route("/auth/sessions/{id}", delete(account::revoke))
        .route("/auth/sessions/revoke-others", post(account::revoke_others))
        .route("/tokens", get(tokens::list).post(tokens::create))
        .route("/tokens/{id}", delete(tokens::delete))
        .route(
            "/brands",
            get(brand_admin::list).post(catalog::create_brand),
        )
        .route("/brands/{id}", patch(brand_admin::update))
        .route("/manual-platforms", get(manual_channels::list))
        .route("/providers", get(catalog::providers))
        .route(
            "/channels",
            get(catalog::channels).post(catalog::create_channel),
        )
        .route("/channels/{id}", patch(catalog::update_channel))
        .route("/ideas", get(ideas::list).post(ideas::create))
        .route("/ideas/{id}", patch(ideas::update).delete(ideas::delete))
        .route(
            "/idea-groups",
            get(idea_groups::list).post(idea_groups::create),
        )
        .route(
            "/idea-groups/{id}",
            patch(idea_groups::update).delete(idea_groups::delete),
        )
        .route("/template-starters", get(templates::starters))
        .route("/templates", get(templates::list).post(templates::create))
        .route(
            "/templates/{id}",
            patch(templates::update).delete(templates::delete),
        )
        .route("/templates/{id}/apply", post(templates::apply))
        .route("/posts", get(posts::list).post(posts::create))
        .route(
            "/posts/{id}",
            get(posts::get).patch(posts::update).delete(posts::delete),
        )
        .route("/posts/{id}/schedule", post(schedules::schedule_post))
        .route(
            "/schedule-requests/{request_id}",
            get(schedules::get_schedule_request),
        )
        .route(
            "/media",
            get(media::list)
                .post(media::upload)
                .layer(DefaultBodyLimit::max(upload_limit)),
        )
        .route("/media/limits", get(media::limits))
        .route(
            "/media/{id}",
            get(media::get).delete(media::delete).patch(media::update),
        )
        .route("/media/{id}/file", get(media::file))
        .route("/schedules", get(schedules::list).post(schedules::create))
        .route("/schedules/{id}", delete(schedules::delete))
        .route("/publications", get(publications::list))
        .route("/publications/{id}/attempts", get(publications::attempts))
        .route("/publications/{id}/cancel", post(publications::cancel))
        .route("/publications/{id}/retry", post(publications::retry))
        .route(
            "/publications/{id}/manual-complete",
            post(publications::manual_complete),
        )
        .route(
            "/publications/{id}/reconcile",
            post(publications::reconcile),
        )
        .route(
            "/publications/{id}/reschedule",
            post(publications::reschedule),
        )
        .route("/publications/{id}/replace", post(queue_actions::replace))
        .route(
            "/publications/{id}/prioritize",
            post(queue_actions::prioritize),
        )
        .route("/queue/reorder", post(publications::reorder))
        .route("/jobs/health", get(health::jobs))
        .route("/analytics", get(health::analytics))
        .route("/inbox", get(health::inbox))
        .fallback(|| async { error::ApiError::missing() });
    let web_dir = std::env::var("BEWEFT_WEB_DIR").unwrap_or_else(|_| "web/dist".into());
    Router::new()
        .nest("/api/v1", api)
        .route("/healthz", get(health::health))
        .route("/api-docs/openapi.json", get(openapi::document))
        .fallback_service(
            ServeDir::new(&web_dir)
                .not_found_service(ServeFile::new(format!("{web_dir}/index.html"))),
        )
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .layer(TraceLayer::new_for_http().make_span_with(|request: &Request| {
            tracing::info_span!("http_request", method=%request.method(), path=%request.uri().path())
        }))
        .layer(middleware::from_fn(security_headers))
        .with_state(state)
}
async fn security_headers(request: Request, next: Next) -> Response {
    let is_api = request.uri().path().starts_with("/api/");
    let mut response = next.run(request).await;
    if is_api
        && response.status().is_client_error()
        && !response
            .headers()
            .get(header::CONTENT_TYPE)
            .is_some_and(|v| v.as_bytes().starts_with(b"application/json"))
    {
        use axum::response::IntoResponse;
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap_or_default();
        let message = String::from_utf8_lossy(&bytes).to_string();
        response = error::ApiError(
            status,
            "invalid_request",
            if message.is_empty() {
                "The request could not be accepted".into()
            } else {
                message
            },
        )
        .into_response();
    }
    let headers = response.headers_mut();
    headers.insert("x-content-type-options", "nosniff".parse().unwrap());
    headers.insert("x-frame-options", "DENY".parse().unwrap());
    headers.insert("referrer-policy", "same-origin".parse().unwrap());
    headers.insert("content-security-policy","default-src 'self'; img-src 'self' data: blob:; media-src 'self' blob:; style-src 'self' 'unsafe-inline'; script-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'".parse().unwrap());
    if is_api {
        headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    }
    response
}
pub fn tracing_init() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "beweft=info,tower_http=info".into()),
        )
        .init();
}
pub async fn shutdown() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("signal handler");
        tokio::select! {_=tokio::signal::ctrl_c()=>{},_=term.recv()=>{}}
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
