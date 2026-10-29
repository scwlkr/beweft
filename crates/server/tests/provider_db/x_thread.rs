async fn x_thread_recovery_and_receipts(h: &Harness) {
    use std::time::Duration;

    let channel = fresh_x(h).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/api/v1", listener.local_addr().unwrap());
    let app = beweft::router(h.state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let cookie = format!("beweft_session={}", h.session);
    let payload = json!({"text":"root", "title":"", "media_ids":[], "options":{"x_thread":{"replies":["reply one","reply two"]}}});
    let status_url = |id: Uuid| format!("{base}/publications/{id}/provider-status");
    let post_url = |id: Uuid, path: &str| format!("{base}/publications/{id}/{path}");

    // A 5xx on reply two leaves the root ID and exact pending boundary durable.
    let (partial, attempt) = sending(h, channel, 300).await;
    sqlx::query("UPDATE publications SET payload=$2 WHERE id=$1")
        .bind(partial)
        .bind(&payload)
        .execute(&h.state.db)
        .await
        .unwrap();
    h.provider.post_mode.store(4, Ordering::SeqCst);
    h.provider.x_thread_sequence.store(0, Ordering::SeqCst);
    h.provider.x_thread_posts.lock().unwrap().clear();
    let before = h.provider.post_writes.load(Ordering::SeqCst);
    let result = provider_bridge::publish(
        &h.state,
        &json!({"id":partial,"attempt_id":attempt,"channel_id":channel}),
    )
    .await;
    let reason = match result {
        provider_bridge::BridgeOutcome::Uncertain { reason } => reason,
        _ => panic!("partial thread must be uncertain"),
    };
    assert!(
        beweft::worker::finish_attempt(
            &h.state,
            partial,
            attempt,
            "uncertain",
            Some(&reason),
            json!({}),
            None
        )
        .await
        .unwrap()
    );
    assert_eq!(h.provider.post_writes.load(Ordering::SeqCst), before + 2);
    assert_eq!(
        *h.provider.x_thread_posts.lock().unwrap(),
        vec![
            json!({"text":"root"}),
            json!({"text":"reply one","reply":{"in_reply_to_tweet_id":"1001"}})
        ]
    );
    let status: Value = client
        .get(status_url(partial))
        .header(header::COOKIE, &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let thread = &status["data"]["x_thread"];
    assert_eq!(thread["phase"], "request_started");
    assert_eq!(thread["confirmed_ids"], json!(["1001"]));
    assert_eq!(thread["next_index"], 1);
    assert_eq!(thread["remaining_texts"], json!(["reply one", "reply two"]));
    assert_eq!(status["data"]["x_upload"]["reset_allowed"], false);
    let observed = json!({"observed_attempt_id":thread["attempt_id"],"observed_checkpoint_id":thread["checkpoint_id"],"sender_stopped":true,"provider_inspected":true});
    let send = |id: Uuid, path: &str, body: Value| {
        client
            .post(post_url(id, path))
            .header(header::COOKIE, &cookie)
            .header("x-beweft-request", "1")
            .json(&body)
    };
    let mut not_published = observed.clone();
    not_published["resolution"] = json!("not_published");
    not_published["note"] = json!("inspected fixture");
    not_published["thread_root_definitively_ended"] = json!(true);
    assert_eq!(
        send(partial, "reconcile", not_published)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    let mut reset = observed.clone();
    reset["confirm_restart"] = json!(true);
    assert_eq!(
        send(partial, "reset-x-upload", reset)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        send(partial, "retry", json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        send(partial, "cancel", json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    let mut complete = observed.clone();
    complete["resolution"] = json!("published");
    complete["note"] = json!("checked full fixture reply chain");
    complete["thread_ids"] = json!(["1001", "1002"]);
    assert_eq!(
        send(partial, "reconcile", complete.clone())
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    complete["thread_ids"] = json!(["9999", "1002", "1003"]);
    assert_eq!(
        send(partial, "reconcile", complete.clone())
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    complete["thread_ids"] = json!(["1001", "1002", "1003"]);
    complete["thread_root_definitively_ended"] = json!(true);
    assert_eq!(
        send(partial, "reconcile", complete.clone())
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    complete
        .as_object_mut()
        .unwrap()
        .remove("thread_root_definitively_ended");
    complete["external_id"] = json!("9999");
    assert_eq!(
        send(partial, "reconcile", complete.clone())
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    complete.as_object_mut().unwrap().remove("external_id");
    let response: Value = send(partial, "reconcile", complete)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(response["data"]["state"], "published");
    assert_eq!(response["data"]["external_id"], "1001");
    assert_eq!(
        response["data"]["external_url"],
        "https://x.com/i/status/1001"
    );
    let receipt: Value = sqlx::query_scalar("SELECT receipt FROM publication_attempts WHERE publication_id=$1 AND receipt->>'verification'='owner_attestation' ORDER BY started_at DESC LIMIT 1").bind(partial).fetch_one(&h.state.db).await.unwrap();
    assert_eq!(
        receipt["detail"]["thread_ids"],
        json!(["1001", "1002", "1003"])
    );
    assert_eq!(h.provider.post_writes.load(Ordering::SeqCst), before + 2);

    // A lost root response may be resolved only after an explicit attestation
    // that the remote operation ended without creating it, then reset/retry.
    let (root, root_attempt) = sending(h, channel, 300).await;
    sqlx::query("UPDATE publications SET payload=$2 WHERE id=$1")
        .bind(root)
        .bind(&payload)
        .execute(&h.state.db)
        .await
        .unwrap();
    h.provider.post_mode.store(5, Ordering::SeqCst);
    h.provider.x_thread_sequence.store(0, Ordering::SeqCst);
    let result = provider_bridge::publish(
        &h.state,
        &json!({"id":root,"attempt_id":root_attempt,"channel_id":channel}),
    )
    .await;
    let reason = match result {
        provider_bridge::BridgeOutcome::Uncertain { reason } => reason,
        _ => panic!("lost root must be uncertain"),
    };
    assert!(
        beweft::worker::finish_attempt(
            &h.state,
            root,
            root_attempt,
            "uncertain",
            Some(&reason),
            json!({}),
            None
        )
        .await
        .unwrap()
    );
    let root_status: Value = client
        .get(status_url(root))
        .header(header::COOKIE, &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let thread = &root_status["data"]["x_thread"];
    assert_eq!(thread["confirmed_ids"], json!([]));
    assert_eq!(thread["root_request_started"], true);
    // An older generic client only attests inspection; it must not authorize
    // reset or retry of the unknown remote root request.
    let mut root_resolution = json!({"resolution":"not_published","note":"looked and found no post","observed_attempt_id":thread["attempt_id"],"observed_checkpoint_id":thread["checkpoint_id"],"sender_stopped":true,"provider_inspected":true});
    assert_eq!(
        send(root, "reconcile", root_resolution.clone())
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    let reset = json!({"observed_attempt_id":thread["attempt_id"],"observed_checkpoint_id":thread["checkpoint_id"],"sender_stopped":true,"provider_inspected":true,"confirm_restart":true});
    assert_eq!(
        send(root, "reset-x-upload", reset.clone())
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        send(root, "retry", json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    let state: String = sqlx::query_scalar("SELECT state FROM publications WHERE id=$1")
        .bind(root)
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    assert_eq!(state, "uncertain");
    root_resolution["thread_root_definitively_ended"] = json!(true);
    root_resolution["provider_inspected"] = json!(false);
    assert_eq!(
        send(root, "reconcile", root_resolution.clone())
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    root_resolution["provider_inspected"] = json!(true);
    assert_eq!(
        send(root, "reconcile", root_resolution)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let root_status: Value = client
        .get(status_url(root))
        .header(header::COOKIE, &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(root_status["data"]["x_upload"]["reset_allowed"], true);
    assert_eq!(
        send(root, "reset-x-upload", reset)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let root_status: Value = client
        .get(status_url(root))
        .header(header::COOKIE, &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(root_status["data"]["x_thread_retry_allowed"], true);
    assert_eq!(
        send(root, "retry", json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let later = Uuid::new_v4();
    sqlx::query("INSERT INTO publication_attempts(id,publication_id,state,provider) VALUES($1,$2,'failed','x')").bind(later).bind(root).execute(&h.state.db).await.unwrap();
    sqlx::query("UPDATE publications SET state='failed' WHERE id=$1")
        .bind(root)
        .execute(&h.state.db)
        .await
        .unwrap();
    let root_status: Value = client
        .get(status_url(root))
        .header(header::COOKIE, &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(root_status["data"]["x_thread_retry_allowed"], false);
    assert_eq!(
        send(root, "retry", json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );

    // Legacy or malformed checkpoints cannot establish a current root write,
    // even if the caller sends the explicit new attestation.
    for checkpoint_body in [
        json!({"stage":"x_post_started","media_ids":[]}),
        json!({"stage":"x_legacy_ambiguous","media_ids":[]}),
        json!({"stage":"x_thread_post_started","media_ids":[],"confirmed_ids":[]}),
        json!({"stage":"x_thread_post_started","media_ids":[],"next_index":0}),
    ] {
        let (legacy, attempt) = sending(h, channel, 300).await;
        sqlx::query(
            "UPDATE publications SET payload=$2,state='uncertain',lease_until=NULL WHERE id=$1",
        )
        .bind(legacy)
        .bind(&payload)
        .execute(&h.state.db)
        .await
        .unwrap();
        sqlx::query("UPDATE publication_attempts SET state='uncertain' WHERE id=$1")
            .bind(attempt)
            .execute(&h.state.db)
            .await
            .unwrap();
        let checkpoint = Uuid::new_v4();
        let mut saved = checkpoint_body;
        saved["attempt_id"] = json!(attempt);
        saved["checkpoint_id"] = json!(checkpoint);
        let encrypted = crypto::encrypt(&[13; 32], legacy, &saved.to_string()).unwrap();
        sqlx::query("INSERT INTO provider_delivery_checkpoints(publication_id,encrypted_checkpoint,attempt_id,checkpoint_id) VALUES($1,$2,$3,$4)").bind(legacy).bind(encrypted).bind(attempt).bind(checkpoint).execute(&h.state.db).await.unwrap();
        let resolution = json!({"resolution":"not_published","note":"root ended","observed_attempt_id":attempt,"observed_checkpoint_id":checkpoint,"sender_stopped":true,"provider_inspected":true,"thread_root_definitively_ended":true});
        assert_eq!(
            send(legacy, "reconcile", resolution)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::CONFLICT
        );
        let reset = json!({"observed_attempt_id":attempt,"observed_checkpoint_id":checkpoint,"sender_stopped":true,"provider_inspected":true,"confirm_restart":true});
        assert_eq!(
            send(legacy, "reset-x-upload", reset)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            send(legacy, "retry", json!({}))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::CONFLICT
        );
        let state: String = sqlx::query_scalar("SELECT state FROM publications WHERE id=$1")
            .bind(legacy)
            .fetch_one(&h.state.db)
            .await
            .unwrap();
        assert_eq!(state, "uncertain");
        if saved["stage"] == "x_post_started" {
            let observed: Value = client
                .get(status_url(legacy))
                .header(header::COOKIE, &cookie)
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            assert_eq!(observed["data"]["x_thread"]["root_request_started"], false);
            let full = json!({"resolution":"published","note":"inspected entire fixture chain","observed_attempt_id":attempt,"observed_checkpoint_id":checkpoint,"sender_stopped":true,"provider_inspected":true,"thread_ids":["7001","7002","7003"]});
            assert_eq!(
                send(legacy, "reconcile", full)
                    .send()
                    .await
                    .unwrap()
                    .status(),
                StatusCode::OK
            );
        }
    }

    // Attestations written before the explicit flag was introduced do not
    // become stronger evidence merely because the software was upgraded.
    let (old_attestation, attempt) = sending(h, channel, 300).await;
    sqlx::query("UPDATE publications SET payload=$2,state='failed',lease_until=NULL WHERE id=$1")
        .bind(old_attestation)
        .bind(&payload)
        .execute(&h.state.db)
        .await
        .unwrap();
    sqlx::query("UPDATE publication_attempts SET state='failed' WHERE id=$1")
        .bind(attempt)
        .execute(&h.state.db)
        .await
        .unwrap();
    let checkpoint = Uuid::new_v4();
    let observed = json!({"attempt_id":attempt,"checkpoint_id":checkpoint});
    let saved = json!({"stage":"x_thread_post_started","media_ids":[],"confirmed_ids":[],"next_index":0,"attempt_id":attempt,"checkpoint_id":checkpoint,"not_published_attestation":observed,"thread_root_ended_attestation":observed});
    let encrypted = crypto::encrypt(&[13; 32], old_attestation, &saved.to_string()).unwrap();
    sqlx::query("INSERT INTO provider_delivery_checkpoints(publication_id,encrypted_checkpoint,attempt_id,checkpoint_id) VALUES($1,$2,$3,$4)").bind(old_attestation).bind(encrypted).bind(attempt).bind(checkpoint).execute(&h.state.db).await.unwrap();
    let reset = json!({"observed_attempt_id":attempt,"observed_checkpoint_id":checkpoint,"sender_stopped":true,"provider_inspected":true,"confirm_restart":true});
    assert_eq!(
        send(old_attestation, "reset-x-upload", reset)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    sqlx::query("DELETE FROM provider_delivery_checkpoints WHERE publication_id=$1")
        .bind(old_attestation)
        .execute(&h.state.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO audit_events(id,owner_id,action,subject_id,detail) VALUES($1,$2,'reset_x_upload',$3,$4)")
        .bind(Uuid::new_v4()).bind(h.auth.owner_id).bind(old_attestation)
        .bind(json!({"thread_safe_reset":true,"attempt_ids":[attempt],"thread_reset_evidence":{"kind":"owner_attested_root_ended","attestation":observed}}))
        .execute(&h.state.db).await.unwrap();
    let status: Value = client
        .get(status_url(old_attestation))
        .header(header::COOKIE, &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status["data"]["x_thread_retry_allowed"], false);
    assert_eq!(
        send(old_attestation, "retry", json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    let youtube: Uuid = sqlx::query_scalar(
        "SELECT id FROM channels WHERE provider='youtube' AND brand_id=$1 LIMIT 1",
    )
    .bind(h.brand)
    .fetch_one(&h.state.db)
    .await
    .unwrap();
    let non_x = h.publication(youtube, "unused").await;
    assert_eq!(send(non_x, "reconcile", json!({"resolution":"not_published","note":"fixture","thread_root_definitively_ended":true})).send().await.unwrap().status(), StatusCode::BAD_REQUEST);

    // A paused claim at a saved reply boundary must not return to the queue.
    let (paused, paused_attempt) = sending(h, channel, 300).await;
    sqlx::query("UPDATE publications SET payload=$2 WHERE id=$1")
        .bind(paused)
        .bind(&payload)
        .execute(&h.state.db)
        .await
        .unwrap();
    let checkpoint = Uuid::new_v4();
    let saved = json!({"stage":"x_thread_ready","media_ids":[],"confirmed_ids":["1001"],"next_index":1,"attempt_id":paused_attempt,"checkpoint_id":checkpoint});
    let encrypted = crypto::encrypt(&[13; 32], paused, &saved.to_string()).unwrap();
    sqlx::query("INSERT INTO provider_delivery_checkpoints(publication_id,encrypted_checkpoint,attempt_id,checkpoint_id) VALUES($1,$2,$3,$4)").bind(paused).bind(encrypted).bind(paused_attempt).bind(checkpoint).execute(&h.state.db).await.unwrap();
    sqlx::query("UPDATE channels SET paused=true WHERE id=$1")
        .bind(channel)
        .execute(&h.state.db)
        .await
        .unwrap();
    let writes = h.provider.post_writes.load(Ordering::SeqCst);
    let result = provider_bridge::publish(
        &h.state,
        &json!({"id":paused,"attempt_id":paused_attempt,"channel_id":channel}),
    )
    .await;
    assert!(matches!(
        result,
        provider_bridge::BridgeOutcome::Uncertain { .. }
    ));
    assert_eq!(h.provider.post_writes.load(Ordering::SeqCst), writes);
    sqlx::query("UPDATE channels SET paused=false WHERE id=$1")
        .bind(channel)
        .execute(&h.state.db)
        .await
        .unwrap();

    // A later claim cannot rebind or resend a public thread checkpoint.
    let (replaced, current_attempt) = sending(h, channel, 300).await;
    sqlx::query("UPDATE publications SET payload=$2 WHERE id=$1")
        .bind(replaced)
        .bind(&payload)
        .execute(&h.state.db)
        .await
        .unwrap();
    let original_attempt = Uuid::new_v4();
    sqlx::query("INSERT INTO publication_attempts(id,publication_id,state,provider) VALUES($1,$2,'uncertain','x')").bind(original_attempt).bind(replaced).execute(&h.state.db).await.unwrap();
    let original_checkpoint = Uuid::new_v4();
    let saved = json!({"stage":"x_thread_post_started","media_ids":[],"confirmed_ids":["1001"],"next_index":1,"attempt_id":original_attempt,"checkpoint_id":original_checkpoint});
    let encrypted = crypto::encrypt(&[13; 32], replaced, &saved.to_string()).unwrap();
    sqlx::query("INSERT INTO provider_delivery_checkpoints(publication_id,encrypted_checkpoint,attempt_id,checkpoint_id) VALUES($1,$2,$3,$4)").bind(replaced).bind(encrypted).bind(original_attempt).bind(original_checkpoint).execute(&h.state.db).await.unwrap();
    let writes = h.provider.post_writes.load(Ordering::SeqCst);
    let result = provider_bridge::publish(
        &h.state,
        &json!({"id":replaced,"attempt_id":current_attempt,"channel_id":channel}),
    )
    .await;
    assert!(matches!(
        result,
        provider_bridge::BridgeOutcome::Uncertain { .. }
    ));
    let stored: String = sqlx::query_scalar(
        "SELECT encrypted_checkpoint FROM provider_delivery_checkpoints WHERE publication_id=$1",
    )
    .bind(replaced)
    .fetch_one(&h.state.db)
    .await
    .unwrap();
    let stored: Value =
        serde_json::from_str(&crypto::decrypt(&[13; 32], replaced, &stored).unwrap()).unwrap();
    assert_eq!(stored["attempt_id"], json!(original_attempt));
    assert_eq!(stored["checkpoint_id"], json!(original_checkpoint));
    assert_eq!(stored["confirmed_ids"], json!(["1001"]));
    assert_eq!(h.provider.post_writes.load(Ordering::SeqCst), writes);

    // Complete provider replies yield one root identity and the full ordered receipt.
    let (complete, attempt) = sending(h, channel, 300).await;
    sqlx::query("UPDATE publications SET payload=$2 WHERE id=$1")
        .bind(complete)
        .bind(&payload)
        .execute(&h.state.db)
        .await
        .unwrap();
    h.provider.post_mode.store(3, Ordering::SeqCst);
    h.provider.x_thread_sequence.store(0, Ordering::SeqCst);
    let result = provider_bridge::publish(
        &h.state,
        &json!({"id":complete,"attempt_id":attempt,"channel_id":channel}),
    )
    .await;
    match result {
        provider_bridge::BridgeOutcome::Published {
            external_id,
            external_url,
            receipt,
        } => {
            assert_eq!(external_id, "1001");
            assert_eq!(external_url.as_deref(), Some("https://x.com/i/status/1001"));
            assert_eq!(receipt["thread_ids"], json!(["1001", "1002", "1003"]));
        }
        _ => panic!("all replies must produce a full provider receipt"),
    }
    h.provider.post_mode.store(0, Ordering::SeqCst);
    server.abort();
    let _ = server.await;
    println!(
        "PASS X thread checkpoint, uncertain reply, owner completion, root reset and full receipt"
    );
}
