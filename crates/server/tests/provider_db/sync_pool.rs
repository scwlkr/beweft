async fn overlapping_syncs_preserve_pool_and_rotated_tokens(h: &Harness) {
    use std::time::Duration;
    h.app("youtube", "sync-client", None).await;
    let brand = Uuid::new_v4();
    let channel = Uuid::new_v4();
    sqlx::query("INSERT INTO brands(id,name) VALUES($1,'Sync pool fixture')")
        .bind(brand)
        .execute(&h.state.db)
        .await
        .unwrap();
    let c = Channel {
        app: connections::app_config(&h.state, "youtube").await.unwrap(),
        identity: Identity {
            id: "sync-destination".into(),
            name: "Fixture".into(),
            username: None,
        },
        tokens: Tokens {
            access_token: "expired".into(),
            refresh_token: Some("rotation-0".into()),
            expires_at: Some(0),
            scopes: vec![],
            scopes_known: false,
        },
    };
    let encrypted =
        crypto::encrypt(&[13; 32], channel, &serde_json::to_string(&c).unwrap()).unwrap();
    sqlx::query("INSERT INTO channels(id,brand_id,name,provider,encrypted_credentials,connection_identity,connection_status) VALUES($1,$2,'Fixture','youtube',$3,$4,'connected')")
        .bind(channel).bind(brand).bind(encrypted).bind(c.destination_key()).execute(&h.state.db).await.unwrap();
    let mut auth = h.auth.clone();
    auth.brand_ids = vec![brand];
    h.provider.strict_refresh.store(true, Ordering::SeqCst);
    h.provider.refresh_generation.store(0, Ordering::SeqCst);
    h.provider.sync_mode.store(1, Ordering::SeqCst);
    let state = h.state.clone();
    let first_auth = auth.clone();
    let first = tokio::spawn(async move {
        provider_bridge::sync(State(state), first_auth, Path(channel), Json(json!({}))).await
    });
    tokio::time::timeout(Duration::from_secs(3), h.provider.sync_started.notified())
        .await
        .unwrap();

    // The harness has a two-connection pool. Nine authenticated HTTP overlaps
    // must all reject while the first provider request is blocked.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new()
        .route("/sync/{id}", axum::routing::post(provider_bridge::sync))
        .with_state(h.state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = reqwest::Client::new();
    let session = format!("beweft_session={}", h.session);
    let mut attempts = tokio::task::JoinSet::new();
    for _ in 0..9 {
        let client = client.clone();
        let session = session.clone();
        let url = format!("{base}/sync/{channel}");
        attempts.spawn(async move {
            client
                .post(url)
                .header(header::COOKIE, session)
                .header("x-beweft-request", "1")
                .json(&json!({}))
                .send()
                .await
                .unwrap()
                .status()
        });
    }
    tokio::time::timeout(Duration::from_secs(3), async {
        while let Some(result) = attempts.join_next().await {
            assert_eq!(result.unwrap(), StatusCode::CONFLICT);
        }
    })
    .await
    .unwrap();
    let probe: i32 = tokio::time::timeout(
        Duration::from_secs(2),
        sqlx::query_scalar("SELECT 1::int").fetch_one(&h.state.db),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(probe, 1);

    // Cancellation rolls back the downstream sync and releases the session lock;
    // the completed refresh rotation was committed separately.
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    let saved = tokio::time::timeout(Duration::from_secs(3), saved_refresh(h, channel))
        .await
        .unwrap();
    assert_eq!(saved, "rotation-1");
    h.provider.sync_mode.store(0, Ordering::SeqCst);
    h.provider.strict_refresh.store(false, Ordering::SeqCst);
    let retry = tokio::time::timeout(
        Duration::from_secs(3),
        provider_bridge::sync(State(h.state.clone()), auth, Path(channel), Json(json!({}))),
    )
    .await
    .unwrap();
    assert!(retry.is_ok());
    assert_eq!(h.provider.refresh_generation.load(Ordering::SeqCst), 1);
    server.abort();

    sqlx::query("DELETE FROM provider_syncs WHERE channel_id=$1")
        .bind(channel)
        .execute(&h.state.db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM channels WHERE id=$1")
        .bind(channel)
        .execute(&h.state.db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM brands WHERE id=$1")
        .bind(brand)
        .execute(&h.state.db)
        .await
        .unwrap();
    println!(
        "PASS nine overlapping syncs reject with pool size two; cancellation retains rotated grant and releases lock"
    );
}
