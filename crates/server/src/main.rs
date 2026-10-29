#[tokio::main]
async fn main() -> anyhow::Result<()> {
    beweft::tracing_init();
    let state = beweft::state::AppState::from_env().await?;
    let bind = std::env::var("BEWEFT_BIND").unwrap_or_else(|_| "0.0.0.0:3000".into());
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!(%bind,"beweft API started");
    axum::serve(listener, beweft::router(state))
        .with_graceful_shutdown(beweft::shutdown())
        .await?;
    Ok(())
}
