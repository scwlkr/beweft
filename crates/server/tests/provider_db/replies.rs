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
    assert!(
        provider_bridge::resolve_reply(
            State(h.state.clone()),
            token_auth,
            Path((inbox, uncertain)),
            Json(json!({"resolution":"not_published","note":"fixture"}))
        )
        .await
        .is_err()
    );
    let result = provider_bridge::resolve_reply(
        State(h.state.clone()),
        h.auth.clone(),
        Path((inbox, uncertain)),
        Json(json!({"resolution":"not_published","note":"Checked fixture network; absent"})),
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
    assert!(
        provider_bridge::resolve_reply(
            State(h.state.clone()),
            h.auth.clone(),
            Path((inbox, request)),
            Json(json!({"resolution":"not_published","note":"too early"}))
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
        while h.provider.writes.load(Ordering::SeqCst) < before + 12 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect(
        "12 valid claims with pool size2 must reach provider without acquiring nested pool slots",
    );
    h.provider.reply_release.notify_waiters();
    for task in tasks {
        let _ = task.await.unwrap().unwrap();
    }
    println!("PASS12 accepted concurrent reply claims with pool size2");
}
