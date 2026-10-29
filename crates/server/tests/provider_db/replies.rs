async fn reply_recovery_and_unresolved_guard(h: &Harness) {
    let channel: Uuid =
        sqlx::query_scalar("SELECT id FROM channels WHERE provider='mastodon' AND brand_id=$1")
            .bind(h.brand)
            .fetch_one(&h.state.db)
            .await
            .unwrap();
    let app = connections::app_config(&h.state, "mastodon").await.unwrap();
    h.save_channel(
        channel,
        &Channel {
            app,
            identity: Identity {
                id: "destination".into(),
                name: "Fixture".into(),
                username: None,
            },
            tokens: Tokens {
                access_token: "current".into(),
                refresh_token: None,
                expires_at: None,
                scopes: vec![],
                scopes_known: false,
            },
        },
    )
    .await;
    let inbox: Uuid =
        sqlx::query_scalar("SELECT id FROM provider_inbox WHERE channel_id=$1 LIMIT 1")
            .bind(channel)
            .fetch_one(&h.state.db)
            .await
            .unwrap();
    let uncertain = Uuid::new_v4();
    sqlx::query("INSERT INTO provider_replies(id,inbox_id,owner_id,text,state,created_at,delivery_deadline) VALUES($1,$2,$3,'lost reply','sending',clock_timestamp()-interval '3 minutes',clock_timestamp()-interval '1 second')").bind(uncertain).bind(inbox).bind(h.auth.owner_id).execute(&h.state.db).await.unwrap();
    let before = h.provider.writes.load(Ordering::SeqCst);
    let rows = provider_bridge::replies(State(h.state.clone()), h.auth.clone(), Path(inbox))
        .await
        .unwrap();
    assert!(
        rows.0["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == uncertain.to_string() && r["state"] == "uncertain")
    );
    let stored: String = sqlx::query_scalar("SELECT state FROM provider_replies WHERE id=$1")
        .bind(uncertain)
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    assert_eq!(stored, "sending", "GET recovery is read-only");
    let mut tasks = vec![];
    for _ in 0..12 {
        let state = h.state.clone();
        let auth = h.auth.clone();
        tasks.push(tokio::spawn(async move {
            provider_bridge::reply(
                State(state),
                auth,
                Path(inbox),
                Json(json!({"text":"new attempt","request_id":Uuid::new_v4(),"expected_last_request_id":uncertain})),
            )
            .await
        }));
    }
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        for task in tasks {
            assert!(task.await.unwrap().is_err());
        }
    })
    .await
    .expect("pool-sized concurrent claims must not deadlock");
    assert_eq!(
        h.provider.writes.load(Ordering::SeqCst),
        before,
        "fresh UUID cannot bypass uncertain reply guard"
    );
    let mut token_auth = h.auth.clone();
    token_auth.session = false;
    token_auth.scopes = vec!["publishing:write".into()];
    let latest = provider_bridge::replies(State(h.state.clone()), h.auth.clone(), Path(inbox)).await.unwrap();
    let revision = latest.0["data"].as_array().unwrap().iter().find(|row| row["id"] == uncertain.to_string()).unwrap()["revision"].as_i64().unwrap();
    assert!(
        provider_bridge::resolve_reply(
            State(h.state.clone()),
            token_auth,
            Path((inbox, uncertain)),
            Json(json!({"resolution":"not_published","note":"fixture","observed_revision":revision,"sender_stopped":true}))
        )
        .await
        .is_err()
    );
    let missing_attestation = provider_bridge::resolve_reply(
        State(h.state.clone()), h.auth.clone(), Path((inbox, uncertain)),
        Json(json!({"resolution":"not_published","note":"fixture","observed_revision":revision})),
    ).await.err().unwrap();
    assert_eq!(missing_attestation.1, "reply_sender_attestation_required");
    let result = provider_bridge::resolve_reply(
        State(h.state.clone()),
        h.auth.clone(),
        Path((inbox, uncertain)),
        Json(json!({"resolution":"not_published","note":"Checked fixture network; absent","observed_revision":revision,"sender_stopped":true})),
    )
    .await
    .unwrap();
    assert_eq!(
        result.0["data"]["receipt"]["verification"],
        "owner_attestation"
    );
    assert_eq!(result.0["data"]["state"], "failed");
    assert_eq!(
        h.provider.writes.load(Ordering::SeqCst),
        before,
        "attestation never resends"
    );
    let request = Uuid::new_v4();
    let state = h.state.clone();
    let auth = h.auth.clone();
    let send = tokio::spawn(async move {
        provider_bridge::reply(
            State(state),
            auth,
            Path(inbox),
            Json(json!({"text":"explicit new send","request_id":request,"expected_last_request_id":uncertain})),
        )
        .await
    });
    h.provider.reply_started.notified().await;
    let active = provider_bridge::replies(State(h.state.clone()), h.auth.clone(), Path(inbox)).await.unwrap();
    let revision = active.0["data"][0]["revision"].as_i64().unwrap();
    assert!(
        provider_bridge::resolve_reply(
            State(h.state.clone()),
            h.auth.clone(),
            Path((inbox, request)),
            Json(json!({"resolution":"not_published","note":"too early","observed_revision":revision,"sender_stopped":true}))
        )
        .await
        .is_err()
    );
    h.provider.reply_release.notify_one();
    let _ = send.await.unwrap().unwrap();
    assert_eq!(h.provider.writes.load(Ordering::SeqCst), before + 1);
    println!(
        "PASS read-only durable reply recovery,12 blocked concurrent new UUIDs, owner attestation and explicit resend"
    );
}

async fn concurrent_reply_claims_with_small_pool(h: &Harness) {
    let channel: Uuid =
        sqlx::query_scalar("SELECT id FROM channels WHERE provider='mastodon' AND brand_id=$1")
            .bind(h.brand)
            .fetch_one(&h.state.db)
            .await
            .unwrap();
    let mut ids = vec![];
    for _ in 0..12 {
        let id = Uuid::new_v4();
        let item = json!({"id":id,"parent_id":null,"author":"person","text":"private","url":null,"created_at":null,"reply_context":{"visibility":"direct","recipients":[{"id":"person","acct":"person@remote.example"}]}});
        sqlx::query(
            "INSERT INTO provider_inbox(id,channel_id,provider_item_id,item) VALUES($1,$2,$3,$4)",
        )
        .bind(id)
        .bind(channel)
        .bind(id.to_string())
        .bind(item)
        .execute(&h.state.db)
        .await
        .unwrap();
        ids.push(id);
    }
    let before = h.provider.writes.load(Ordering::SeqCst);
    let mut tasks = vec![];
    for id in ids {
        let state = h.state.clone();
        let auth = h.auth.clone();
        tasks.push(tokio::spawn(async move {
            provider_bridge::reply(
                State(state),
                auth,
                Path(id),
                Json(json!({"text":"concurrent fixture","request_id":Uuid::new_v4()})),
            )
            .await
        }));
    }
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while h.provider.writes.load(Ordering::SeqCst) < before + 1 || tasks.iter().filter(|task| task.is_finished()).count() < 11 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("excess replies must fail promptly without occupying the reserved pool slot");
    let probe: i32 = sqlx::query_scalar("SELECT 1").fetch_one(&h.state.db).await.unwrap();
    assert_eq!(probe, 1);
    let logout_token = crypto::random_secret();
    sqlx::query("INSERT INTO sessions(token_hash,owner_id,expires_at) VALUES($1,$2,clock_timestamp()+interval '1 hour')")
        .bind(crypto::digest(&logout_token)).bind(h.auth.owner_id).execute(&h.state.db).await.unwrap();
    let mut logout_headers = HeaderMap::new();
    logout_headers.insert(header::COOKIE, format!("beweft_session={logout_token}").parse().unwrap());
    tokio::time::timeout(std::time::Duration::from_secs(2),
        beweft::auth::logout(State(h.state.clone()), h.auth.clone(), logout_headers))
        .await.expect("sign-out must retain pool capacity").unwrap();
    let busy_sync = provider_bridge::sync(State(h.state.clone()), h.auth.clone(), Path(channel), Json(json!({}))).await.err().unwrap();
    assert_eq!(busy_sync.0, StatusCode::CONFLICT);
    h.provider.reply_release.notify_one();
    let mut accepted = 0;
    let mut busy = 0;
    for task in tasks {
        match task.await.unwrap() {
            Ok(_) => accepted += 1,
            Err(error) if error.0 == StatusCode::CONFLICT => busy += 1,
            Err(error) => panic!("unexpected error: {error:?}"),
        }
    }
    assert_eq!((accepted, busy), (1, 11));
    assert_eq!(h.provider.writes.load(Ordering::SeqCst), before + 1);
    // Cancel a held handler. Its session guard and shared permit must be
    // released, but cancellation does not prove the provider saw no request.
    let canceled_inbox = Uuid::new_v4();
    let canceled_request = Uuid::new_v4();
    let item = json!({"id":canceled_inbox,"parent_id":null,"author":"person","text":"private","url":null,"created_at":null,"reply_context":{"visibility":"direct","recipients":[{"id":"person","acct":"person@remote.example"}]}});
    sqlx::query("INSERT INTO provider_inbox(id,channel_id,provider_item_id,item) VALUES($1,$2,$3,$4)")
        .bind(canceled_inbox).bind(channel).bind(canceled_inbox.to_string()).bind(item)
        .execute(&h.state.db).await.unwrap();
    let state = h.state.clone();
    let auth = h.auth.clone();
    let canceled = tokio::spawn(async move {
        provider_bridge::reply(State(state), auth, Path(canceled_inbox),
            Json(json!({"text":"cancel fixture","request_id":canceled_request}))).await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while h.provider.writes.load(Ordering::SeqCst) < before + 2 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    canceled.abort();
    let _ = canceled.await;
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let result = provider_bridge::reply(State(h.state.clone()), h.auth.clone(), Path(canceled_inbox),
                Json(json!({"text":"new attempt","request_id":Uuid::new_v4(),"expected_last_request_id":canceled_request}))).await;
            let error = result.expect_err("an unresolved canceled reply cannot be resent");
            if error.2.contains("earlier reply") { break; }
            assert!(error.1 == "reply_execution_active" || error.2.contains("capacity is busy"), "unexpected error: {error:?}");
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }).await.expect("canceling must release the permit and session guard");
    assert_eq!(h.provider.writes.load(Ordering::SeqCst), before + 2);
    h.provider.reply_release.notify_waiters();
    sqlx::query("DELETE FROM provider_replies WHERE inbox_id=$1").bind(canceled_inbox).execute(&h.state.db).await.unwrap();
    sqlx::query("DELETE FROM provider_inbox WHERE id=$1").bind(canceled_inbox).execute(&h.state.db).await.unwrap();
    println!("PASS shared reply/sync admission reserves pool capacity with 12 concurrent reply claims");
}
