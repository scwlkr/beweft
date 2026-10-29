async fn expiring_grant(h: &Harness, id: Uuid, token: Option<&str>) {
    let encrypted: String =
        sqlx::query_scalar("SELECT encrypted_credentials FROM channels WHERE id=$1")
            .bind(id)
            .fetch_one(&h.state.db)
            .await
            .unwrap();
    let mut c: Channel =
        serde_json::from_str(&crypto::decrypt(&[13; 32], id, &encrypted).unwrap()).unwrap();
    c.tokens.expires_at = Some(0);
    if let Some(token) = token {
        c.tokens.refresh_token = Some(token.into());
    }
    h.save_channel(id, &c).await;
}
async fn saved_refresh(h: &Harness, id: Uuid) -> String {
    let encrypted: String =
        sqlx::query_scalar("SELECT encrypted_credentials FROM channels WHERE id=$1")
            .bind(id)
            .fetch_one(&h.state.db)
            .await
            .unwrap();
    let c: Channel =
        serde_json::from_str(&crypto::decrypt(&[13; 32], id, &encrypted).unwrap()).unwrap();
    c.tokens.refresh_token.unwrap()
}
async fn refresh_survives_downstream_rollback(h: &Harness) {
    let channel: Uuid =
        sqlx::query_scalar("SELECT id FROM channels WHERE provider='youtube' AND brand_id=$1")
            .bind(h.brand)
            .fetch_one(&h.state.db)
            .await
            .unwrap();
    h.provider.strict_refresh.store(true, Ordering::SeqCst);
    h.provider.refresh_generation.store(0, Ordering::SeqCst);
    for (index, session) in ["missing", "unqueryable", "unqueryable-again"]
        .into_iter()
        .enumerate()
    {
        expiring_grant(
            h,
            channel,
            if index == 0 { Some("rotation-0") } else { None },
        )
        .await;
        let id = h.publication(channel, session).await;
        if session == "missing" {
            sqlx::query("DELETE FROM provider_delivery_checkpoints WHERE publication_id=$1")
                .bind(id)
                .execute(&h.state.db)
                .await
                .unwrap();
        }
        let outcome =
            provider_bridge::reconcile(State(h.state.clone()), h.auth.clone(), Path(id)).await;
        if session == "missing" {
            assert!(outcome.is_err());
        } else {
            assert_eq!(outcome.unwrap().0["data"]["state"], "uncertain");
        }
        assert_eq!(
            saved_refresh(h, channel).await,
            format!("rotation-{}", index + 1),
            "downstream failure/inconclusive probe cannot roll back successful rotation"
        );
    }
    // A worker's media failure and a later network deadline likewise cannot discard the new refresh token.
    let x = fresh_x(h).await;
    h.provider.refresh_generation.store(0, Ordering::SeqCst);
    expiring_grant(h, x, Some("rotation-0")).await;
    let (id, attempt) = sending(h, x, 300).await;
    sqlx::query("UPDATE publications SET payload=jsonb_set(payload,'{media_ids}',$2) WHERE id=$1")
        .bind(id)
        .bind(json!([Uuid::new_v4()]))
        .execute(&h.state.db)
        .await
        .unwrap();
    assert!(matches!(
        provider_bridge::publish(
            &h.state,
            &json!({"id":id,"attempt_id":attempt,"channel_id":x})
        )
        .await,
        provider_bridge::BridgeOutcome::Uncertain { .. }
    ));
    assert_eq!(saved_refresh(h, x).await, "rotation-1");
    expiring_grant(h, x, None).await;
    h.provider.post_mode.store(2, Ordering::SeqCst);
    let (id, attempt) = sending(h, x, 2).await;
    assert!(matches!(
        provider_bridge::publish(
            &h.state,
            &json!({"id":id,"attempt_id":attempt,"channel_id":x})
        )
        .await,
        provider_bridge::BridgeOutcome::Uncertain { .. }
    ));
    assert_eq!(saved_refresh(h, x).await, "rotation-2");
    expiring_grant(h, x, None).await;
    connections::load_channel(&h.state, x).await.unwrap();
    assert_eq!(saved_refresh(h, x).await, "rotation-3");
    h.provider.strict_refresh.store(false, Ordering::SeqCst);
    h.provider.post_mode.store(0, Ordering::SeqCst);
    println!(
        "PASS single-use refresh rotation survives missing checkpoint, inconclusive probes, missing media and fenced timeout"
    );
}
async fn provider_fetches_committed_media_during_post(h: &Harness) {
    h.app("facebook", "fb-client", None).await;
    let channel = h.channel("facebook").await;
    let id = h.publication(channel, "unused").await;
    // The shared publication helper seeds a YouTube upload checkpoint. A new
    // Facebook post has no such checkpoint and must not inherit another provider's session.
    sqlx::query("DELETE FROM provider_delivery_checkpoints WHERE publication_id=$1")
        .bind(id).execute(&h.state.db).await.unwrap();
    let attempt = Uuid::new_v4();
    let media: Value =
        sqlx::query_scalar("SELECT payload->'media_ids' FROM publications WHERE id=$1")
            .bind(id)
            .fetch_one(&h.state.db)
            .await
            .unwrap();
    let media_id: Uuid = media[0].as_str().unwrap().parse().unwrap();
    let storage: String = sqlx::query_scalar("SELECT storage_key FROM media WHERE id=$1")
        .bind(media_id)
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    let bytes = media_fixture::fixture_bytes(false);
    tokio::fs::write(h.state.config.media_dir.join(storage), &bytes)
        .await
        .unwrap();
    sqlx::query("UPDATE media SET mime='image/jpeg',name='fixture.jpg',size=$2 WHERE id=$1")
        .bind(media_id)
        .bind(bytes.len() as i64)
        .execute(&h.state.db)
        .await
        .unwrap();
    sqlx::query("UPDATE publications SET state='sending',lease_until=clock_timestamp()+interval '5 minutes' WHERE id=$1").bind(id).execute(&h.state.db).await.unwrap();
    sqlx::query("INSERT INTO publication_attempts(id,publication_id,state,provider) VALUES($1,$2,'sending','facebook')").bind(attempt).bind(id).execute(&h.state.db).await.unwrap();
    let old = &h.state.config;
    let mut state = h.state.clone();
    state.config = Arc::new(Config {
            public_url: "https://media.fixture".into(),
            secure_cookie: true,
            setup_secret: String::new(),
            media_dir: old.media_dir.clone(),
            fixture_enabled: true,
            max_upload_bytes: old.max_upload_bytes,
            media_quota_bytes: old.media_quota_bytes,
            fixture_delay_ms: 0,
            encryption_key: old.encryption_key,
        });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    *h.provider.callback_origin.lock().unwrap() =
        format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .route(
            "/api/v1/provider-media/{id}",
            axum::routing::get(provider_bridge::media_file),
        )
        .with_state(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let result = provider_bridge::publish(
        &state,
        &json!({"id":id,"attempt_id":attempt,"channel_id":channel}),
    )
    .await;
    server.abort();
    let _ = server.await;
    assert!(
        matches!(result, provider_bridge::BridgeOutcome::Published { .. }),
        "provider POST waits for actual HTTP media callback before it can succeed"
    );
    assert_eq!(*h.provider.media_received.lock().unwrap(), bytes);
    println!(
        "PASS provider fetches exact committed media bytes over HTTP DURING publishing POST with pool size2"
    );
}
