async fn replies_and_disconnect_share_claim_lock(h: &Harness) {
    h.app("mastodon", "masto-client", Some("https://first.example"))
        .await;
    let id: Uuid =
        sqlx::query_scalar("SELECT id FROM channels WHERE provider='mastodon' AND brand_id=$1")
            .bind(h.brand)
            .fetch_one(&h.state.db)
            .await
            .unwrap();
    sqlx::query("UPDATE channels SET connection_status='connected' WHERE id=$1")
        .bind(id)
        .execute(&h.state.db)
        .await
        .unwrap();
    let inbox = Uuid::new_v4();
    let item = json!({"id":"original","parent_id":null,"author":"person","text":"private","url":null,"created_at":null,"reply_context":{"visibility":"direct","recipients":[{"id":"person","acct":"person@remote.example"}]}});
    sqlx::query("INSERT INTO provider_inbox(id,channel_id,provider_item_id,item) VALUES($1,$2,'original',$3)").bind(inbox).bind(id).bind(item).execute(&h.state.db).await.unwrap();
    let state = h.state.clone();
    let auth = h.auth.clone();
    let request = Uuid::new_v4();
    let sending = tokio::spawn(async move {
        provider_bridge::reply(
            State(state),
            auth,
            Path(inbox),
            Json(json!({"text":"secret reply","request_id":request})),
        )
        .await
    });
    h.provider.reply_started.notified().await;
    assert!(
        connections::disconnect(State(h.state.clone()), h.auth.clone(), Path(id))
            .await
            .is_err(),
        "disconnect must see durable active reply"
    );
    h.provider.reply_release.notify_one();
    let _ = sending.await.unwrap().unwrap();
    let _ = provider_bridge::reply(
        State(h.state.clone()),
        h.auth.clone(),
        Path(inbox),
        Json(json!({"text":"secret reply","request_id":request})),
    )
    .await
    .unwrap();
    assert_eq!(
        h.provider.writes.load(Ordering::SeqCst),
        1,
        "same request must not send twice"
    );
    let _ = connections::disconnect(State(h.state.clone()), h.auth.clone(), Path(id))
        .await
        .unwrap();
    assert!(
        provider_bridge::reply(
            State(h.state.clone()),
            h.auth.clone(),
            Path(inbox),
            Json(json!({"text":"late reply","request_id":Uuid::new_v4()}))
        )
        .await
        .is_err()
    );
    assert_eq!(h.provider.writes.load(Ordering::SeqCst), 1);
    println!("PASS disconnect/reply claim serialization and duplicate-request suppression");
}
async fn reconciliation_serialization_and_expired_reset(h: &Harness) {
    h.app("youtube", "google", None).await;
    let channel = h.channel("youtube").await;
    let id = h.publication(channel, "session").await;
    let state = h.state.clone();
    let auth = h.auth.clone();
    let first =
        tokio::spawn(async move { provider_bridge::reconcile(State(state), auth, Path(id)).await });
    h.provider.probe_started.notified().await;
    let state = h.state.clone();
    let auth = h.auth.clone();
    let second =
        tokio::spawn(async move { provider_bridge::reconcile(State(state), auth, Path(id)).await });
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    assert!(!second.is_finished());
    h.provider.probe_release.notify_one();
    let _ = first.await.unwrap().unwrap();
    assert!(
        second.await.unwrap().is_err(),
        "stale concurrent probe must not overwrite queued progress"
    );
    let row=sqlx::query("SELECT p.state,cp.encrypted_checkpoint FROM publications p JOIN provider_delivery_checkpoints cp ON cp.publication_id=p.id WHERE p.id=$1").bind(id).fetch_one(&h.state.db).await.unwrap();
    assert_eq!(row.get::<String, _>("state"), "queued");
    let cp: Value = serde_json::from_str(
        &crypto::decrypt(&[13; 32], id, row.get("encrypted_checkpoint")).unwrap(),
    )
    .unwrap();
    assert_eq!(cp["offset"], 5);
    let old_attempt = Uuid::new_v4();
    sqlx::query("UPDATE publications SET state='sending',lease_until=now()+interval '5 minutes' WHERE id=$1").bind(id).execute(&h.state.db).await.unwrap();
    sqlx::query("INSERT INTO publication_attempts(id,publication_id,state,provider) VALUES($1,$2,'sending','youtube')").bind(old_attempt).bind(id).execute(&h.state.db).await.unwrap();
    let mut tx = h.state.db.begin().await.unwrap();
    sqlx::query("SELECT id FROM publications WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    let state = h.state.clone();
    let stale = tokio::spawn(async move {
        provider_bridge::publish(
            &state,
            &json!({"id":id,"attempt_id":old_attempt,"channel_id":channel}),
        )
        .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    sqlx::query("UPDATE publication_attempts SET state='uncertain' WHERE id=$1")
        .bind(old_attempt)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO publication_attempts(id,publication_id,state,provider) VALUES($1,$2,'sending','youtube')").bind(Uuid::new_v4()).bind(id).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert!(
        matches!(
            stale.await.unwrap(),
            provider_bridge::BridgeOutcome::Uncertain { .. }
        ),
        "old generation cannot save a checkpoint after waiting for a newer attempt"
    );
    let encrypted: String = sqlx::query_scalar(
        "SELECT encrypted_checkpoint FROM provider_delivery_checkpoints WHERE publication_id=$1",
    )
    .bind(id)
    .fetch_one(&h.state.db)
    .await
    .unwrap();
    let saved: Value =
        serde_json::from_str(&crypto::decrypt(&[13; 32], id, &encrypted).unwrap()).unwrap();
    assert_eq!(saved["offset"], 5);
    let expired = h.publication(channel, "expired").await;
    let out = provider_bridge::reconcile(State(h.state.clone()), h.auth.clone(), Path(expired))
        .await
        .unwrap();
    assert_eq!(out.0["data"]["session_expired"], true);
    assert!(
        provider_bridge::reset_expired_session(
            State(h.state.clone()),
            h.auth.clone(),
            Path(expired)
        )
        .await
        .is_err()
    );
    let _ = beweft::publications::reconcile(
        State(h.state.clone()),
        h.auth.clone(),
        Path(expired),
        Json(
            serde_json::from_value(
                json!({"resolution":"not_published","note":"Inspected isolated fixture outcome"}),
            )
            .unwrap(),
        ),
    )
    .await
    .unwrap();
    let _ = provider_bridge::reset_expired_session(
        State(h.state.clone()),
        h.auth.clone(),
        Path(expired),
    )
    .await
    .unwrap();
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM provider_delivery_checkpoints WHERE publication_id=$1",
    )
    .bind(expired)
    .fetch_one(&h.state.db)
    .await
    .unwrap();
    assert_eq!(count, 0);
    println!(
        "PASS atomic checkpoint progress and explicit expired-session reset after owner reconciliation"
    );
}
