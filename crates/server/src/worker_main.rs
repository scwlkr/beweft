mod worker_health;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let healthcheck = match (args.next(), args.next()) {
        (None, None) => false,
        (Some(flag), None) if flag == "--healthcheck" => true,
        _ => anyhow::bail!("Usage: beweft-worker [--healthcheck]"),
    };
    let configured_id = worker_health::configured_id()?;
    if healthcheck {
        let id = configured_id
            .ok_or_else(|| anyhow::anyhow!("BEWEFT_WORKER_ID is required for a healthcheck"))?;
        let database_url = std::env::var("DATABASE_URL")
            .ok()
            .filter(|url| !url.is_empty())
            .ok_or_else(|| anyhow::anyhow!("DATABASE_URL is required for a healthcheck"))?;
        // SAFETY: main has not initialized a runtime, tracing, a database
        // connection, or any worker thread. Regular worker mode skips this.
        unsafe { worker_health::isolate_postgres_environment() };
        return worker_health::probe(database_url, id);
    }
    let id = configured_id.unwrap_or_else(|| format!("worker-{}", uuid::Uuid::new_v4()));
    beweft::tracing_init();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let state = beweft::state::AppState::from_env().await?;
        beweft::worker::run(state, id).await
    })
}
