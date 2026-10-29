async fn bluesky_connection_obeys_provider_and_brand_fences(h: &Harness) {
    use axum::routing::{patch, post, put};
    use std::time::Duration;

    h.app("bluesky", "", Some("https://bsky.social")).await;
    let brand = Uuid::new_v4();
    let old_channel = Uuid::new_v4();
    sqlx::query("INSERT INTO brands(id,name) VALUES($1,'Bluesky fence fixture')")
        .bind(brand)
        .execute(&h.state.db)
        .await
        .unwrap();
    let old = Channel {
        app: connections::app_config(&h.state, "bluesky").await.unwrap(),
        identity: Identity {
            id: "did:plc:old".into(),
            name: "Old".into(),
            username: None,
        },
        tokens: Tokens {
            access_token: "old-bluesky-access".into(),
            refresh_token: Some("old-bluesky-refresh".into()),
            expires_at: None,
            scopes: vec![],
            scopes_known: false,
        },
    };
    let encrypted = crypto::encrypt(
        &[13; 32],
        old_channel,
        &serde_json::to_string(&old).unwrap(),
    )
    .unwrap();
    sqlx::query("INSERT INTO channels(id,brand_id,name,provider,encrypted_credentials,connection_identity,connection_status) VALUES($1,$2,'Old','bluesky',$3,$4,'connected')")
        .bind(old_channel).bind(brand).bind(encrypted).bind(old.destination_key()).execute(&h.state.db).await.unwrap();
    let router = Router::new()
        .route("/connect", post(connections::bluesky))
        .route("/disconnect/{id}", post(connections::disconnect))
        .route("/ack/{provider}", post(connections::acknowledge_revocation))
        .route("/app/{provider}", put(connections::save_app))
        .route("/brand/{id}", patch(beweft::brand_admin::update))
        .with_state(h.state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let session = format!("beweft_session={}", h.session);
    let connect = |reconnect: Option<Uuid>| {
        client.post(format!("{base}/connect"))
        .header(header::COOKIE, &session).header("x-beweft-request", "1")
        .json(&json!({"brand_id":brand,"identifier":"fixture.bsky.social","app_password":"bluesky-password-fixture","reconnect_channel_id":reconnect}))
    };
    let disconnect = || {
        client
            .post(format!("{base}/disconnect/{old_channel}"))
            .header(header::COOKIE, &session)
            .header("x-beweft-request", "1")
    };
    let ack = |revocation_id: Uuid| {
        client
            .post(format!("{base}/ack/bluesky"))
            .header(header::COOKIE, &session)
            .header("x-beweft-request", "1")
            .json(&json!({"access_removed":true,"revocation_id":revocation_id}))
    };
    let archive = |value: bool| {
        client
            .patch(format!("{base}/brand/{brand}"))
            .header(header::COOKIE, &session)
            .header("x-beweft-request", "1")
            .json(&json!({"archived":value}))
    };
    let app = |origin: &str| {
        client
            .put(format!("{base}/app/bluesky"))
            .header(header::COOKIE, &session)
            .header("x-beweft-request", "1")
            .json(&json!({"client_id":"","instance_url":origin}))
    };

    // Disconnect leaves provider access for owner-managed removal and leaves
    // a durable pending fence. A new app-password request must not be sent.
    let result: Value = disconnect().send().await.unwrap().json().await.unwrap();
    assert_eq!(result["data"]["provider_revoked"], false);
    let before = h.provider.bluesky_session_requests.load(Ordering::SeqCst);
    assert_eq!(
        connect(None).send().await.unwrap().status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        h.provider.bluesky_session_requests.load(Ordering::SeqCst),
        before
    );
    assert_eq!(
        app("https://bsky.social").send().await.unwrap().status(),
        StatusCode::OK
    );
    assert_eq!(
        connect(None).send().await.unwrap().status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        h.provider.bluesky_session_requests.load(Ordering::SeqCst),
        before
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM channels WHERE brand_id=$1")
        .bind(brand)
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(ack(h.revocation_marker("bluesky").await).send().await.unwrap().status(), StatusCode::OK);

    // Validate the reconnect target and brand scope before the provider write.
    assert_eq!(
        connect(Some(Uuid::new_v4())).send().await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    let denied = connections::bluesky(State(h.state.clone()), h.auth.clone(), h.headers(), Json(json!({"brand_id":brand,"identifier":"fixture","app_password":"bluesky-password-fixture"}))).await.unwrap_err();
    assert_eq!(denied.0, StatusCode::FORBIDDEN);
    assert_eq!(
        h.provider.bluesky_session_requests.load(Ordering::SeqCst),
        before
    );

    h.provider.bluesky_session_block.store(1, Ordering::SeqCst);
    let active = tokio::spawn(connect(None).send());
    tokio::time::timeout(
        Duration::from_secs(3),
        h.provider.bluesky_session_started.notified(),
    )
    .await
    .unwrap();
    assert_eq!(
        connect(None).send().await.unwrap().status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        disconnect().send().await.unwrap().status(),
        StatusCode::CONFLICT
    );
    assert_eq!(ack(h.revocation_marker("bluesky").await).send().await.unwrap().status(), StatusCode::CONFLICT);
    assert_eq!(
        h.provider.bluesky_session_requests.load(Ordering::SeqCst),
        before + 1
    );

    // Archive can complete while the provider is stalled. The brand recheck
    // then rolls back the new connection instead of storing an active channel.
    let archiving = tokio::time::timeout(Duration::from_secs(2), archive(true).send())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(archiving.status(), StatusCode::OK);
    let archived: bool =
        sqlx::query_scalar("SELECT archived_at IS NOT NULL FROM brands WHERE id=$1")
            .bind(brand)
            .fetch_one(&h.state.db)
            .await
            .unwrap();
    assert!(archived);
    h.provider.bluesky_session_block.store(0, Ordering::SeqCst);
    h.provider.bluesky_session_release.notify_one();
    let response = active.await.unwrap().unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = response.text().await.unwrap();
    assert!(body.contains("First inspect your connected channels"));
    assert!(body.contains("session may already exist"));
    for secret in [
        "bluesky-password-fixture",
        "bluesky-access-fixture",
        "bluesky-refresh-fixture",
        &h.session,
    ] {
        assert!(
            !body.contains(secret),
            "connection response exposed a secret"
        );
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM channels WHERE brand_id=$1")
        .bind(brand)
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    assert_eq!(count, 1);
    let writes = h.provider.bluesky_session_requests.load(Ordering::SeqCst);
    assert_eq!(
        connect(None).send().await.unwrap().status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        h.provider.bluesky_session_requests.load(Ordering::SeqCst),
        writes
    );
    assert_eq!(
        archive(false).send().await.unwrap().status(),
        StatusCode::OK
    );
    let response = connect(None).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().await.unwrap();
    for secret in [
        "bluesky-password-fixture",
        "bluesky-access-fixture",
        "bluesky-refresh-fixture",
        &h.session,
    ] {
        assert!(
            !body.contains(secret),
            "connection response exposed a secret"
        );
    }
    let value: Value = serde_json::from_str(&body).unwrap();
    let connected: Uuid = value["data"]["id"].as_str().unwrap().parse().unwrap();

    // A config change cannot move an in-flight session to a different PDS.
    h.provider.bluesky_session_block.store(1, Ordering::SeqCst);
    let active = tokio::spawn(connect(Some(connected)).send());
    tokio::time::timeout(
        Duration::from_secs(3),
        h.provider.bluesky_session_started.notified(),
    )
    .await
    .unwrap();
    let changes: Vec<_> = (0..6)
        .map(|_| tokio::spawn(app("https://other.example").send()))
        .collect();
    for changing in changes {
        let response = tokio::time::timeout(Duration::from_secs(3), changing)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }
    let probe: i32 = tokio::time::timeout(
        Duration::from_secs(2),
        sqlx::query_scalar("SELECT 1").fetch_one(&h.state.db),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(probe, 1);
    h.provider.bluesky_session_block.store(0, Ordering::SeqCst);
    h.provider.bluesky_session_release.notify_one();
    assert_eq!(active.await.unwrap().unwrap().status(), StatusCode::OK);
    assert_eq!(
        app("https://other.example").send().await.unwrap().status(),
        StatusCode::OK
    );
    let encrypted: String =
        sqlx::query_scalar("SELECT encrypted_credentials FROM channels WHERE id=$1")
            .bind(connected)
            .fetch_one(&h.state.db)
            .await
            .unwrap();
    let saved: Channel =
        serde_json::from_str(&crypto::decrypt(&[13; 32], connected, &encrypted).unwrap()).unwrap();
    assert_eq!(
        saved.app.instance_url.as_deref(),
        Some("https://bsky.social")
    );
    assert_eq!(
        connections::app_config(&h.state, "bluesky")
            .await
            .unwrap()
            .instance_url
            .as_deref(),
        Some("https://other.example")
    );
    assert_eq!(
        app("https://bsky.social").send().await.unwrap().status(),
        StatusCode::OK
    );

    // A near-expiry browser session cannot begin a remote session creation.
    sqlx::query("UPDATE sessions SET expires_at=clock_timestamp()+interval '0.9 seconds' WHERE token_hash=$1")
        .bind(crypto::digest(&h.session)).execute(&h.state.db).await.unwrap();
    let before_near = h.provider.bluesky_session_requests.load(Ordering::SeqCst);
    let near = connect(Some(connected)).send().await.unwrap();
    assert_eq!(near.status(), StatusCode::CONFLICT);
    let message = near.text().await.unwrap();
    assert!(message.contains("before the provider request") && message.contains("First inspect your connected channels"));
    assert_eq!(h.provider.bluesky_session_requests.load(Ordering::SeqCst), before_near);
    sqlx::query("UPDATE sessions SET expires_at=clock_timestamp()+interval '1 hour' WHERE token_hash=$1")
        .bind(crypto::digest(&h.session)).execute(&h.state.db).await.unwrap();

    // If the originating browser session expires or is revoked during the
    // external call, the stored grant update rolls back in both cases.
    for revoked in [false, true] {
        let before_grant: String =
            sqlx::query_scalar("SELECT encrypted_credentials FROM channels WHERE id=$1")
                .bind(connected)
                .fetch_one(&h.state.db)
                .await
                .unwrap();
        h.provider.bluesky_session_block.store(1, Ordering::SeqCst);
        let active = tokio::spawn(connect(Some(connected)).send());
        tokio::time::timeout(
            Duration::from_secs(3),
            h.provider.bluesky_session_started.notified(),
        )
        .await
        .unwrap();
        if revoked {
            sqlx::query("DELETE FROM sessions WHERE token_hash=$1")
                .bind(crypto::digest(&h.session))
                .execute(&h.state.db)
                .await
                .unwrap();
        } else {
            sqlx::query("UPDATE sessions SET expires_at=clock_timestamp()-interval '1 second' WHERE token_hash=$1").bind(crypto::digest(&h.session)).execute(&h.state.db).await.unwrap();
        }
        h.provider.bluesky_session_block.store(0, Ordering::SeqCst);
        h.provider.bluesky_session_release.notify_one();
        let response = active.await.unwrap().unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = response.text().await.unwrap();
        assert!(body.contains("First inspect your connected channels"));
        for secret in [
            "bluesky-password-fixture",
            "bluesky-access-fixture",
            "bluesky-refresh-fixture",
            &h.session,
        ] {
            assert!(!body.contains(secret), "recovery response exposed a secret");
        }
        let after_grant: String =
            sqlx::query_scalar("SELECT encrypted_credentials FROM channels WHERE id=$1")
                .bind(connected)
                .fetch_one(&h.state.db)
                .await
                .unwrap();
        assert_eq!(before_grant, after_grant);
        sqlx::query("INSERT INTO sessions(token_hash,owner_id,expires_at) VALUES($1,$2,now()+interval '1 hour') ON CONFLICT(token_hash) DO UPDATE SET expires_at=EXCLUDED.expires_at")
            .bind(crypto::digest(&h.session)).bind(h.auth.owner_id).execute(&h.state.db).await.unwrap();
    }

    sqlx::query("DELETE FROM audit_events WHERE subject_id=$1")
        .bind(brand)
        .execute(&h.state.db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM channels WHERE brand_id=$1")
        .bind(brand)
        .execute(&h.state.db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM brands WHERE id=$1")
        .bind(brand)
        .execute(&h.state.db)
        .await
        .unwrap();
    server.abort();
    println!(
        "PASS Bluesky pending revocation, in-flight fence, app authority, brand archival and browser-session recovery"
    );
}
