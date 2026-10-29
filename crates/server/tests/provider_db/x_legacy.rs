async fn legacy_x_recovery_uses_observed_identity_without_provider_writes(h: &Harness) {
    use std::time::Duration;
    // Exercise the upgrade with a populated pre-0014 checkpoint, rather than
    // only migrating an empty database.
    let mut upgrade = h.state.db.begin().await.unwrap();
    sqlx::raw_sql("CREATE TEMP TABLE provider_delivery_checkpoints(publication_id uuid PRIMARY KEY,encrypted_checkpoint text NOT NULL,updated_at timestamptz NOT NULL DEFAULT now()) ON COMMIT DROP; CREATE TEMP TABLE publication_attempts(id uuid PRIMARY KEY,publication_id uuid NOT NULL,started_at timestamptz NOT NULL DEFAULT now(),receipt jsonb) ON COMMIT DROP")
        .execute(&mut *upgrade).await.unwrap();
    let historical_publication = Uuid::new_v4();
    let historical_attempt = Uuid::new_v4();
    sqlx::query("INSERT INTO provider_delivery_checkpoints(publication_id,encrypted_checkpoint) VALUES($1,'legacy')")
        .bind(historical_publication).execute(&mut *upgrade).await.unwrap();
    sqlx::query("INSERT INTO publication_attempts(id,publication_id) VALUES($1,$2)")
        .bind(historical_attempt).bind(historical_publication).execute(&mut *upgrade).await.unwrap();
    sqlx::raw_sql(include_str!("../../../../migrations/0014_provider_checkpoint_timing.sql"))
        .execute(&mut *upgrade).await.unwrap();
    let upgraded: (Uuid, Option<Uuid>) = sqlx::query_as("SELECT checkpoint_id,attempt_id FROM provider_delivery_checkpoints WHERE publication_id=$1")
        .bind(historical_publication).fetch_one(&mut *upgrade).await.unwrap();
    assert_ne!(upgraded.0, Uuid::nil());
    assert_eq!(upgraded.1, Some(historical_attempt));
    upgrade.rollback().await.unwrap();
    let channel = fresh_x(h).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/api/v1", listener.local_addr().unwrap());
    let app = beweft::router(h.state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::builder().timeout(Duration::from_secs(5)).build().unwrap();
    let cookie = format!("beweft_session={}", h.session);
    let before = h.provider.post_writes.load(Ordering::SeqCst);
    for ids in [json!(["123456789"]), json!(["123456789", "123456790", "123456791", "123456792"])] {
        let publication = h.publication(channel, "unused").await;
        let attempt = Uuid::new_v4();
        let checkpoint = Uuid::new_v4();
        let legacy = crypto::encrypt(&[13;32], publication, &json!({"stage":"media","media_ids":ids}).to_string()).unwrap();
        sqlx::query("UPDATE provider_delivery_checkpoints SET encrypted_checkpoint=$2,attempt_id=$3,checkpoint_id=$4 WHERE publication_id=$1")
            .bind(publication).bind(legacy).bind(attempt).bind(checkpoint).execute(&h.state.db).await.unwrap();
        sqlx::query("INSERT INTO publication_attempts(id,publication_id,state,provider,finished_at) VALUES($1,$2,'uncertain','x',now())")
            .bind(attempt).bind(publication).execute(&h.state.db).await.unwrap();
        let get = || client.get(format!("{base}/publications/{publication}/provider-status")).header(header::COOKIE, &cookie);
        let status: Value = get().send().await.unwrap().json().await.unwrap();
        assert_eq!(status["data"]["x_upload"]["phase"], "post_started");
        assert_eq!(status["data"]["x_upload"]["attempt_id"], json!(attempt));
        assert_eq!(status["data"]["x_upload"]["checkpoint_id"], json!(checkpoint));
        assert_eq!(status["data"]["x_upload"]["reset_allowed"], false);
        let body = json!({"observed_attempt_id":attempt,"observed_checkpoint_id":checkpoint,"sender_stopped":true,"confirm_restart":true,"provider_inspected":true});
        let post = |path: &str, value: Value| client.post(format!("{base}/publications/{publication}/{path}"))
            .header(header::COOKIE, &cookie).header("x-beweft-request", "1").json(&value);
        assert_eq!(post("reset-x-upload", body.clone()).send().await.unwrap().status(), StatusCode::CONFLICT);
        let resolution = json!({"resolution":"not_published","note":"Fixture inspected","observed_attempt_id":attempt,"observed_checkpoint_id":checkpoint,"sender_stopped":true,"provider_inspected":true});
        assert_eq!(post("reconcile", json!({"resolution":"not_published","note":"Fixture inspected","observed_attempt_id":attempt,"observed_checkpoint_id":Uuid::new_v4(),"sender_stopped":true,"provider_inspected":true})).send().await.unwrap().status(), StatusCode::CONFLICT);
        assert_eq!(post("reconcile", resolution.clone()).send().await.unwrap().status(), StatusCode::OK);
        let status: Value = get().send().await.unwrap().json().await.unwrap();
        assert_eq!(status["data"]["x_upload"]["reset_allowed"], true);
        assert_eq!(post("retry", json!({})).send().await.unwrap().status(), StatusCode::CONFLICT);
        if ids.as_array().unwrap().len() == 1 {
            // A replacement attempt changes the checkpoint identity and clears
            // the previous attestation without issuing a new public request.
            let replacement = Uuid::new_v4();
            sqlx::query("UPDATE publications SET state='sending',lease_until=clock_timestamp()+interval '5 minutes' WHERE id=$1")
                .bind(publication).execute(&h.state.db).await.unwrap();
            sqlx::query("INSERT INTO publication_attempts(id,publication_id,state,provider) VALUES($1,$2,'sending','x')")
                .bind(replacement).bind(publication).execute(&h.state.db).await.unwrap();
            let outcome = provider_bridge::publish(&h.state, &json!({"id":publication,"attempt_id":replacement,"channel_id":channel})).await;
            assert!(matches!(outcome, provider_bridge::BridgeOutcome::Uncertain { .. }));
            assert!(beweft::worker::finish_attempt(&h.state, publication, replacement, "uncertain", None, json!({}), None).await.unwrap());
            let status: Value = get().send().await.unwrap().json().await.unwrap();
            assert_eq!(status["data"]["x_upload"]["attempt_id"], json!(replacement));
            assert_ne!(status["data"]["x_upload"]["checkpoint_id"], json!(checkpoint));
            assert_eq!(post("reconcile", resolution).send().await.unwrap().status(), StatusCode::CONFLICT);
            assert_eq!(post("reset-x-upload", body).send().await.unwrap().status(), StatusCode::CONFLICT);
        } else {
            assert_eq!(post("reset-x-upload", body).send().await.unwrap().status(), StatusCode::OK);
            assert_eq!(post("retry", json!({})).send().await.unwrap().status(), StatusCode::OK);
        }
    }
    assert_eq!(h.provider.post_writes.load(Ordering::SeqCst), before);
    server.abort();
    let _ = server.await;
    println!("PASS legacy X full/partial image quarantine, exact-ID recovery and stale attestation fencing");
}
