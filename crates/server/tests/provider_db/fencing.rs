async fn fresh_x(h: &Harness) -> Uuid {
    h.app("x", "client", None).await;
    let id: Uuid =
        sqlx::query_scalar("SELECT id FROM channels WHERE provider='x' AND brand_id=$1 LIMIT 1")
            .bind(h.brand)
            .fetch_one(&h.state.db)
            .await
            .unwrap();
    let c = Channel {
        app: connections::app_config(&h.state, "x").await.unwrap(),
        identity: Identity {
            parent: None,
            id: "destination".into(),
            name: "Fixture".into(),
            username: None,
        },
        tokens: Tokens {
            access_token: "current-grant".into(),
            refresh_token: None,
            expires_at: None,
            scopes: vec![],
            scopes_known: false,
        },
    };
    h.save_channel(id, &c).await;
    id
}
async fn sending(h: &Harness, channel: Uuid, lease_seconds: i32) -> (Uuid, Uuid) {
    let id = h.publication(channel, "unused").await;
    let attempt = Uuid::new_v4();
    sqlx::query("UPDATE publications SET state='sending',lease_until=clock_timestamp()+make_interval(secs=>$2),payload='{\"text\":\"fixture post\",\"media_ids\":[],\"options\":{}}' WHERE id=$1").bind(id).bind(lease_seconds as f64).execute(&h.state.db).await.unwrap();
    sqlx::query("DELETE FROM provider_delivery_checkpoints WHERE publication_id=$1")
        .bind(id)
        .execute(&h.state.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO publication_attempts(id,publication_id,state,provider) VALUES($1,$2,'sending','x')").bind(attempt).bind(id).execute(&h.state.db).await.unwrap();
    (id, attempt)
}
async fn fenced_writes_and_small_pool(h: &Harness) {
    let channel = fresh_x(h).await;
    let (expired, attempt) = sending(h, channel, -1).await;
    let result = provider_bridge::publish(
        &h.state,
        &json!({"id":expired,"attempt_id":attempt,"channel_id":channel}),
    )
    .await;
    assert!(matches!(
        result,
        provider_bridge::BridgeOutcome::Uncertain { .. }
    ));
    assert!(!beweft::worker::finish_attempt(
        &h.state, expired, attempt, "published", None,
        json!({"late":"receipt"}), Some(("late-id", Some("https://example.invalid/late")))
    ).await.unwrap());
    let expired_state: (String, Option<String>, Option<Value>) = sqlx::query_as(
        "SELECT p.state,p.external_id,a.receipt FROM publications p JOIN publication_attempts a ON a.publication_id=p.id WHERE p.id=$1 AND a.id=$2"
    ).bind(expired).bind(attempt).fetch_one(&h.state.db).await.unwrap();
    assert_eq!(expired_state, ("sending".into(), None, None));
    let (replaced, old) = sending(h, channel, 300).await;
    sqlx::query("UPDATE publication_attempts SET state='uncertain' WHERE id=$1")
        .bind(old)
        .execute(&h.state.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO publication_attempts(id,publication_id,state,provider) VALUES($1,$2,'sending','x')").bind(Uuid::new_v4()).bind(replaced).execute(&h.state.db).await.unwrap();
    let result = provider_bridge::publish(
        &h.state,
        &json!({"id":replaced,"attempt_id":old,"channel_id":channel}),
    )
    .await;
    assert!(matches!(
        result,
        provider_bridge::BridgeOutcome::Uncertain { .. }
    ));
    assert!(!beweft::worker::finish_attempt(
        &h.state, replaced, old, "published", None,
        json!({"late":"receipt"}), Some(("late-id", Some("https://example.invalid/late")))
    ).await.unwrap());
    let replacement_state: (String, Option<String>, Option<Value>) = sqlx::query_as(
        "SELECT p.state,p.external_id,a.receipt FROM publications p JOIN publication_attempts a ON a.publication_id=p.id WHERE p.id=$1 AND a.id=$2"
    ).bind(replaced).bind(old).fetch_one(&h.state.db).await.unwrap();
    assert_eq!(replacement_state, ("sending".into(), None, None));
    assert_eq!(
        h.provider.post_writes.load(Ordering::SeqCst),
        0,
        "expired and replaced attempts must make ZERO requests even for a text-only single-step adapter"
    );
    // A media stage held beyond its lease may have succeeded remotely. It
    // cannot store a checkpoint or complete the expired local attempt.
    let (expiring, expiring_attempt) = sending(h, channel, 30).await;
    let media = Uuid::new_v4();
    let bytes = media_fixture::fixture_bytes(false);
    tokio::fs::write(h.state.config.media_dir.join(media.to_string()), &bytes).await.unwrap();
    sqlx::query("INSERT INTO media(id,brand_id,name,mime,size,storage_key) VALUES($1,$2,'fixture.jpg','image/jpeg',$3,$4)")
        .bind(media).bind(h.brand).bind(bytes.len() as i64).bind(media.to_string())
        .execute(&h.state.db).await.unwrap();
    sqlx::query("UPDATE publications SET payload=$2,lease_until=clock_timestamp()+interval '5 seconds' WHERE id=$1")
        .bind(expiring).bind(json!({"text":"fixture image","title":"","media_ids":[media],"options":{}}))
        .execute(&h.state.db).await.unwrap();
    h.provider.media_block.store(1, Ordering::SeqCst);
    let state = h.state.clone();
    let delivery = tokio::spawn(async move {
        provider_bridge::publish(&state, &json!({"id":expiring,"attempt_id":expiring_attempt,"channel_id":channel})).await
    });
    tokio::time::timeout(std::time::Duration::from_secs(4), h.provider.media_started.notified()).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(5200)).await;
    h.provider.media_release.notify_one();
    assert!(matches!(delivery.await.unwrap(), provider_bridge::BridgeOutcome::Uncertain { .. }));
    h.provider.media_block.store(0, Ordering::SeqCst);
    let checkpoints: i64 = sqlx::query_scalar("SELECT count(*) FROM provider_delivery_checkpoints WHERE publication_id=$1")
        .bind(expiring).fetch_one(&h.state.db).await.unwrap();
    // The empty media checkpoint predates the held request. The late response
    // cannot advance it or store a provider media ID.
    assert_eq!(checkpoints, 1);
    let encrypted: String = sqlx::query_scalar("SELECT encrypted_checkpoint FROM provider_delivery_checkpoints WHERE publication_id=$1")
        .bind(expiring).fetch_one(&h.state.db).await.unwrap();
    let saved: Value = serde_json::from_str(&crypto::decrypt(&[13;32], expiring, &encrypted).unwrap()).unwrap();
    assert_eq!(saved["stage"], "x_image_media");
    assert_eq!(saved["media_ids"], json!([]));
    assert!(!beweft::worker::finish_attempt(
        &h.state, expiring, expiring_attempt, "published", None,
        json!({"late":"receipt"}), Some(("late-id", None))
    ).await.unwrap());
    let state: (String, Option<String>, Option<Value>) = sqlx::query_as(
        "SELECT p.state,p.external_id,a.receipt FROM publications p JOIN publication_attempts a ON a.publication_id=p.id WHERE p.id=$1 AND a.id=$2"
    ).bind(expiring).bind(expiring_attempt).fetch_one(&h.state.db).await.unwrap();
    assert_eq!(state, ("sending".into(), None, None));
    let (good, attempt) = sending(h, channel, 300).await;
    let result = provider_bridge::publish(
        &h.state,
        &json!({"id":good,"attempt_id":attempt,"channel_id":channel}),
    )
    .await;
    assert!(matches!(
        result,
        provider_bridge::BridgeOutcome::Published { .. }
    ));
    assert_eq!(
        h.provider.post_writes.load(Ordering::SeqCst),
        1,
        "control must demonstrate reachable publishing fixture"
    );
    h.provider.post_mode.store(2, Ordering::SeqCst);
    let (slow, attempt) = sending(h, channel, 2).await;
    let started = std::time::Instant::now();
    let result = provider_bridge::publish(
        &h.state,
        &json!({"id":slow,"attempt_id":attempt,"channel_id":channel}),
    )
    .await;
    assert!(matches!(
        result,
        provider_bridge::BridgeOutcome::Uncertain { .. }
    ));
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "request deadline must fit remaining lease"
    );
    h.provider.post_mode.store(1, Ordering::SeqCst);
    let (rejected, attempt) = sending(h, channel, 300).await;
    let flow = Uuid::new_v4();
    let new = Channel {
        app: connections::app_config(&h.state, "x").await.unwrap(),
        identity: Identity {
            parent: None,
            id: "destination".into(),
            name: "Fixture".into(),
            username: None,
        },
        tokens: Tokens {
            access_token: "fresh-reconnected-token".into(),
            refresh_token: None,
            expires_at: None,
            scopes: vec![],
            scopes_known: false,
        },
    };
    let encrypted =
        crypto::encrypt(&[13; 32], flow, &serde_json::to_string(&vec![new]).unwrap()).unwrap();
    sqlx::query("INSERT INTO connection_flows(id,provider,owner_id,brand_id,session_hash,state_hash,correlation_hash,encrypted_verifier,reconnect_channel_id,expires_at,encrypted_candidates,oauth_generation) VALUES($1,'x',$2,$3,$4,$5,'unused','unused',$6,now()+interval '10 minutes',$7,COALESCE((SELECT oauth_generation FROM provider_oauth_fences WHERE provider='x'),0))").bind(flow).bind(h.auth.owner_id).bind(h.brand).bind(crypto::digest(&h.session)).bind(flow.to_string()).bind(channel).bind(encrypted).execute(&h.state.db).await.unwrap();
    let state = h.state.clone();
    let publisher = tokio::spawn(async move {
        provider_bridge::publish(
            &state,
            &json!({"id":rejected,"attempt_id":attempt,"channel_id":channel}),
        )
        .await
    });
    h.provider.post_started.notified().await;
    let state = h.state.clone();
    let auth = h.auth.clone();
    let headers = h.headers();
    let reconnect = tokio::spawn(async move {
        connections::complete(
            State(state),
            auth,
            headers,
            Path(flow),
            Json(json!({"destination_id":"destination"})),
        )
        .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    assert!(!reconnect.is_finished());
    h.provider.post_release.notify_one();
    assert!(matches!(
        publisher.await.unwrap(),
        provider_bridge::BridgeOutcome::Rejected { .. }
    ));
    let _ = reconnect.await.unwrap().unwrap();
    let current = connections::load_channel(&h.state, channel).await.unwrap();
    assert_eq!(
        current.tokens.access_token, "fresh-reconnected-token",
        "late401 cannot mark new grant reconnect-required"
    );
    h.provider.post_mode.store(0, Ordering::SeqCst);
    let youtube: Uuid =
        sqlx::query_scalar("SELECT id FROM channels WHERE provider='youtube' AND brand_id=$1")
            .bind(h.brand)
            .fetch_one(&h.state.db)
            .await
            .unwrap();
    let mut tasks = vec![];
    for _ in 0..12 {
        let id = h.publication(youtube, "fastsession").await;
        let state = h.state.clone();
        let auth = h.auth.clone();
        tasks.push(tokio::spawn(async move {
            provider_bridge::reconcile(State(state), auth, Path(id)).await
        }));
    }
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        for task in tasks {
            let _ = task.await.unwrap().unwrap();
        }
    })
    .await
    .expect("12 reconciliations with pool size2 cannot need a nested connection");
    println!(
        "PASS zero stale-worker writes, lease-bound HTTP deadline, reconnect/401 serialization and12 reconciliations with pool size2"
    );
}
