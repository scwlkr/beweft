async fn meta_recovery_channel(h: &Harness, provider: &str) -> Uuid {
    h.app(provider, "meta-recovery-client", None).await;
    let id = Uuid::new_v4();
    h.save_channel(id, &Channel {
        app: connections::app_config(&h.state, provider).await.unwrap(),
        identity: Identity { parent:None, id:format!("meta-recovery-{provider}"), name:"Fixture".into(), username:None },
        tokens: Tokens { access_token:"fixture-token".into(), refresh_token:None, expires_at:None, scopes:vec![], scopes_known:false },
    }).await;
    id
}

async fn meta_public_recovery_requires_new_remote_end_attestation(h: &Harness) {
    let facebook = meta_recovery_channel(h, "facebook").await;
    let instagram = meta_recovery_channel(h, "instagram").await;
    let threads = meta_recovery_channel(h, "threads").await;
    for (provider, stage, channel) in [
        ("facebook", "fb_video_processing", facebook),
        ("facebook", "fb_reel_processing", facebook),
        ("instagram", "meta_public", instagram),
        ("threads", "meta_public", threads),
    ] {
        let id = h.publication(channel, "unused").await;
        let attempt = Uuid::new_v4();
        let checkpoint = Uuid::new_v4();
        sqlx::query("INSERT INTO publication_attempts(id,publication_id,state,provider,finished_at) VALUES($1,$2,'uncertain',$3,now())")
            .bind(attempt).bind(id).bind(provider).execute(&h.state.db).await.unwrap();
        let cp = json!({"stage":stage,"attempt_id":attempt,"checkpoint_id":checkpoint,
            "video_id":"12345","container_id":"12345",
            "not_published_attestation":{"attempt_id":attempt,"checkpoint_id":checkpoint}});
        let encrypted = crypto::encrypt(&[13; 32], id, &cp.to_string()).unwrap();
        sqlx::query("UPDATE provider_delivery_checkpoints SET encrypted_checkpoint=$2,attempt_id=$3,checkpoint_id=$4 WHERE publication_id=$1")
            .bind(id).bind(encrypted).bind(attempt).bind(checkpoint).execute(&h.state.db).await.unwrap();
        sqlx::query("UPDATE publications SET state='failed' WHERE id=$1")
            .bind(id).execute(&h.state.db).await.unwrap();
        let status = provider_bridge::provider_status(State(h.state.clone()), h.auth.clone(), Path(id)).await.unwrap();
        assert_eq!(status.0["data"]["meta_delivery"]["retry_allowed"], false);
        assert!(beweft::publications::retry(State(h.state.clone()), h.auth.clone(), Path(id)).await.is_err());
        let base = json!({"resolution":"not_published","note":"The fixture remote operation ended",
            "observed_attempt_id":attempt,"observed_checkpoint_id":checkpoint,
            "sender_stopped":true,"provider_inspected":true});
        let resolve = |body: Value| beweft::publications::reconcile(
            State(h.state.clone()), h.auth.clone(), Path(id),
            Json(serde_json::from_value(body).unwrap()));
        assert!(resolve(base.clone()).await.is_err(), "old inspection is not remote-ended proof");
        let mut stale = base.clone();
        stale["observed_checkpoint_id"] = json!(Uuid::new_v4());
        stale["remote_definitively_ended"] = json!(true);
        assert!(resolve(stale).await.is_err(), "stale checkpoint must not be accepted");
        let mut confirmed = base;
        confirmed["remote_definitively_ended"] = json!(true);
        let result = resolve(confirmed).await.unwrap();
        assert_eq!(result.0["data"]["state"], "failed", "re-attestation must not enqueue");
        let encrypted: String = sqlx::query_scalar("SELECT encrypted_checkpoint FROM provider_delivery_checkpoints WHERE publication_id=$1")
            .bind(id).fetch_one(&h.state.db).await.unwrap();
        let saved: Value = serde_json::from_str(&crypto::decrypt(&[13; 32], id, &encrypted).unwrap()).unwrap();
        assert_eq!(saved["attempt_id"], json!(attempt));
        assert_eq!(saved["checkpoint_id"], json!(checkpoint));
        assert_eq!(saved["not_published_attestation"]["remote_definitively_ended"], true);
        let status = provider_bridge::provider_status(State(h.state.clone()), h.auth.clone(), Path(id)).await.unwrap();
        assert_eq!(status.0["data"]["meta_delivery"]["retry_allowed"], true);
        let retry = beweft::publications::retry(State(h.state.clone()), h.auth.clone(), Path(id)).await.unwrap();
        assert_eq!(retry.0["data"]["state"], "queued");
    }
    let id = h.publication(instagram, "unused").await;
    let attempt = Uuid::new_v4();
    let checkpoint = Uuid::new_v4();
    sqlx::query("INSERT INTO publication_attempts(id,publication_id,state,provider,finished_at) VALUES($1,$2,'uncertain','instagram',now())")
        .bind(attempt).bind(id).execute(&h.state.db).await.unwrap();
    let cp = json!({"stage":"meta_ready","attempt_id":attempt,"checkpoint_id":checkpoint,"container_id":"12345"});
    let encrypted = crypto::encrypt(&[13; 32], id, &cp.to_string()).unwrap();
    sqlx::query("UPDATE provider_delivery_checkpoints SET encrypted_checkpoint=$2,attempt_id=$3,checkpoint_id=$4 WHERE publication_id=$1")
        .bind(id).bind(encrypted).bind(attempt).bind(checkpoint).execute(&h.state.db).await.unwrap();
    let body = json!({"resolution":"not_published","note":"Private container inspected", "observed_attempt_id":attempt,
        "observed_checkpoint_id":checkpoint,"sender_stopped":true,"provider_inspected":true});
    let _ = beweft::publications::reconcile(State(h.state.clone()), h.auth.clone(), Path(id), Json(serde_json::from_value(body).unwrap())).await.unwrap();
    let status = provider_bridge::provider_status(State(h.state.clone()), h.auth.clone(), Path(id)).await.unwrap();
    assert_eq!(status.0["data"]["meta_delivery"]["phase"], "upload");
    assert_eq!(status.0["data"]["meta_delivery"]["retry_allowed"], true);
    println!("PASS public Meta legacy re-attestation, stale IDs and explicit retry");
}
