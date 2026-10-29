async fn first_provider_save_rejects_concurrent_creators(h: &Harness) {
    use axum::routing::put;
    use std::time::{Duration, Instant};

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(Duration::from_secs(3))
        .connect(&std::env::var("BEWEFT_TEST_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let state = AppState::new(pool.clone(), h.state.config.clone());
    let router = Router::new()
        .route("/app/{provider}", put(connections::save_app))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .unwrap();
    let session = format!("beweft_session={}", h.session);
    let save = |provider: &str| {
        client
            .put(format!("{base}/app/{provider}"))
            .header(header::COOKIE, &session)
            .header("x-beweft-request", "1")
            .json(&json!({"client_id":"fixture-client","client_secret":"gate-secret-fixture"}))
    };

    // Pause the first save after it creates its uncommitted fence, without
    // consuming a connection from the application pool under test.
    let mut blocker = h.state.db.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind("provider-app:pinterest")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let first = tokio::spawn(save("pinterest").send());
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE 'SELECT pg_advisory_xact_lock(hashtext%')")
                .fetch_one(&h.state.db).await.unwrap();
            if waiting { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    let began = Instant::now();
    let competing: Vec<_> = (0..6)
        .map(|_| tokio::spawn(save("pinterest").send()))
        .collect();
    for request in competing {
        let response = tokio::time::timeout(Duration::from_secs(2), request)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = response.text().await.unwrap();
        assert!(!body.contains("gate-secret-fixture"));
        assert!(body.contains("in progress"));
    }
    assert!(began.elapsed() < Duration::from_secs(3));
    let probe: i32 = tokio::time::timeout(
        Duration::from_secs(2),
        sqlx::query_scalar("SELECT 1").fetch_one(&pool),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(probe, 1);
    assert_eq!(
        save("linkedin").send().await.unwrap().status(),
        StatusCode::OK
    );
    blocker.rollback().await.unwrap();
    assert_eq!(first.await.unwrap().unwrap().status(), StatusCode::OK);
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM provider_oauth_fences WHERE provider='pinterest'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 1);

    // An already-created fence also retains the ordinary NOWAIT behavior.
    let mut existing = h.state.db.begin().await.unwrap();
    sqlx::query("SELECT provider FROM provider_oauth_fences WHERE provider='pinterest' FOR UPDATE")
        .fetch_one(&mut *existing)
        .await
        .unwrap();
    let response = tokio::time::timeout(Duration::from_secs(2), save("pinterest").send())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    existing.rollback().await.unwrap();
    sqlx::query("DELETE FROM provider_apps WHERE provider IN ('pinterest','linkedin')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM provider_oauth_fences WHERE provider IN ('pinterest','linkedin')")
        .execute(&pool)
        .await
        .unwrap();
    server.abort();
    pool.close().await;
    println!(
        "PASS first-provider save rejects competing creators promptly and leaves pool capacity"
    );
}
