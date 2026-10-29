async fn delayed_reply_cannot_bypass_terminal_history(h: &Harness) {
    let channel: Uuid =
        sqlx::query_scalar("SELECT id FROM channels WHERE provider='mastodon' AND brand_id=$1")
            .bind(h.brand)
            .fetch_one(&h.state.db)
            .await
            .unwrap();
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
    let before = h.provider.writes.load(Ordering::SeqCst);
    // Tabs A and B both read empty history. A's first request is delayed before reaching the server.
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let state = h.state.clone();
    let auth = h.auth.clone();
    let first = tokio::spawn(async move {
        provider_bridge::reply(
            State(state),
            auth,
            Path(id),
            Json(json!({"text":"tab B reply","request_id":b,"expected_last_request_id":null})),
        )
        .await
    });
    // Previously completed stress test may have left a notification permit; wait on the actual write count.
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while h.provider.writes.load(Ordering::SeqCst) < before + 1 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    h.provider.reply_release.notify_one();
    let row = first.await.unwrap().unwrap();
    assert_eq!(row.0["data"]["state"], "published");
    // B is already terminal, so an unresolved-only guard would now incorrectly allow A.
    let delayed = provider_bridge::reply(
        State(h.state.clone()),
        h.auth.clone(),
        Path(id),
        Json(json!({"text":"delayed tab A reply","request_id":a,"expected_last_request_id":null})),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(delayed.0, StatusCode::CONFLICT);
    assert_eq!(delayed.1, "reply_history_changed");
    // Omitting the watermark means null; it is never an escape hatch.
    let omitted = provider_bridge::reply(
        State(h.state.clone()),
        h.auth.clone(),
        Path(id),
        Json(json!({"text":"omitted watermark","request_id":Uuid::new_v4()})),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(omitted.1, "reply_history_changed");
    let duplicate = provider_bridge::reply(
        State(h.state.clone()),
        h.auth.clone(),
        Path(id),
        Json(json!({"text":"tab B retry","request_id":b,"expected_last_request_id":null})),
    )
    .await
    .unwrap();
    assert_eq!(duplicate.0["data"]["id"], b.to_string());
    let history = provider_bridge::replies(State(h.state.clone()), h.auth.clone(), Path(id))
        .await
        .unwrap();
    assert_eq!(history.0["data"][0]["id"], b.to_string());
    assert_eq!(history.0["data"].as_array().unwrap().len(), 1);
    assert_eq!(
        h.provider.writes.load(Ordering::SeqCst),
        before + 1,
        "delayed tab and repeated UUID must not make extra provider writes"
    );
    println!(
        "PASS delayed tab A after completed tab B rejected by durable history watermark; one provider write total"
    );
}
