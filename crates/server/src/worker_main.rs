#[tokio::main]
async fn main() -> anyhow::Result<()> {
    beweft::tracing_init();
    let state = beweft::state::AppState::from_env().await?;
    beweft::worker::run(state).await
}
