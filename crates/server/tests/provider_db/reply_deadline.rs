async fn reply_claim_clock_is_independent_of_waits(h: &Harness) {
    let channel: Uuid =
        sqlx::query_scalar("SELECT id FROM channels WHERE provider='mastodon' AND brand_id=$1")
            .bind(h.brand)
            .fetch_one(&h.state.db)
            .await
            .unwrap();
    let id = Uuid::new_v4();
    let request = Uuid::new_v4();
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
    // Deterministic substitute for an old transaction-start timestamp after a long lock wait.
    // The production INSERT must override the stale default with the actual claim clock.
    sqlx::query("ALTER TABLE provider_replies ALTER COLUMN created_at SET DEFAULT (clock_timestamp()-interval '3 minutes')").execute(&h.state.db).await.unwrap();
    let before = h.provider.writes.load(Ordering::SeqCst);
    let mut blocker = h.state.db.begin().await.unwrap();
    beweft::schedules::lock_channel(&mut blocker, channel)
        .await
        .unwrap();
    let state = h.state.clone();
    let auth = h.auth.clone();
    let send = tokio::spawn(async move {
        provider_bridge::reply(
            State(state),
            auth,
            Path(id),
            Json(json!({"text":"after wait","request_id":request,"expected_last_request_id":null})),
        )
        .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(!send.is_finished());
    blocker.commit().await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while h.provider.writes.load(Ordering::SeqCst) < before + 1 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let row=sqlx::query("SELECT created_at>clock_timestamp()-interval '5 seconds' AS fresh,delivery_deadline>clock_timestamp()+interval '80 seconds' AS live FROM provider_replies WHERE id=$1").bind(request).fetch_one(&h.state.db).await.unwrap();
    assert!(row.get::<bool, _>("fresh") && row.get::<bool, _>("live"));
    // Even a stale display timestamp cannot expire an active HTTP request's stored deadline.
    sqlx::query(
        "UPDATE provider_replies SET created_at=clock_timestamp()-interval '3 minutes' WHERE id=$1",
    )
    .bind(request)
    .execute(&h.state.db)
    .await
    .unwrap();
    let history = provider_bridge::replies(State(h.state.clone()), h.auth.clone(), Path(id))
        .await
        .unwrap();
    assert_eq!(history.0["data"][0]["state"], "sending");
    assert!(
        provider_bridge::resolve_reply(
            State(h.state.clone()),
            h.auth.clone(),
            Path((id, request)),
            Json(json!({"resolution":"not_published","note":"must not accept while HTTP active"}))
        )
        .await
        .is_err()
    );
    assert!(provider_bridge::reply(State(h.state.clone()),h.auth.clone(),Path(id),Json(json!({"text":"duplicate","request_id":Uuid::new_v4(),"expected_last_request_id":request}))).await.is_err());
    assert!(
        connections::disconnect(State(h.state.clone()), h.auth.clone(), Path(channel))
            .await
            .is_err()
    );
    assert_eq!(h.provider.writes.load(Ordering::SeqCst), before + 1);
    h.provider.reply_release.notify_one();
    let _ = send.await.unwrap().unwrap();
    sqlx::query(
        "ALTER TABLE provider_replies ALTER COLUMN created_at SET DEFAULT clock_timestamp()",
    )
    .execute(&h.state.db)
    .await
    .unwrap();
    println!(
        "PASS fresh claim clock and durable HTTP deadline survive lock wait/stale timestamp; active reply cannot resolve or duplicate"
    );
}
