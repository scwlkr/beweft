async fn bluesky_quota_retry_preserves_identity_and_requires_a_finished_sender(h: &Harness) {
    let channel = Uuid::new_v4();
    sqlx::query("INSERT INTO channels(id,brand_id,name,provider) VALUES($1,$2,'Quota fixture','bluesky')")
        .bind(channel).bind(h.brand).execute(&h.state.db).await.unwrap();
    let publication = h.publication(channel, "unused").await;
    let attempt = Uuid::new_v4();
    let checkpoint = Uuid::new_v4();
    let original = json!({
        "stage":"bsky_video_upload_rejected",
        "rkey":"3quotaidentity",
        "created_at":"2026-10-29T20:00:00Z",
        "blobs":[],
        "attempt_id":attempt,
        "checkpoint_id":checkpoint,
        "pds_audience":"did:web:pds.fixture"
    });
    let encrypted = crypto::encrypt(&[13; 32], publication, &original.to_string()).unwrap();
    sqlx::query("UPDATE publications SET state='failed' WHERE id=$1").bind(publication).execute(&h.state.db).await.unwrap();
    sqlx::query("INSERT INTO publication_attempts(id,publication_id,state,provider,error,receipt,finished_at) VALUES($1,$2,'uncertain','bluesky',$3,$4,clock_timestamp())")
        .bind(attempt).bind(publication).bind(beweft_providers::BLUESKY_VIDEO_RATE_LIMIT)
        .bind(json!({"execution":"direct","verification":"rejected"})).execute(&h.state.db).await.unwrap();
    sqlx::query("UPDATE provider_delivery_checkpoints SET encrypted_checkpoint=$2,attempt_id=$3,checkpoint_id=$4 WHERE publication_id=$1")
        .bind(publication).bind(encrypted).bind(attempt).bind(checkpoint).execute(&h.state.db).await.unwrap();
    let payload: Value = sqlx::query_scalar("SELECT payload FROM publications WHERE id=$1").bind(publication).fetch_one(&h.state.db).await.unwrap();

    // A lost worker completion is not a durable rejection, even if the local
    // owner has made the publication failed by some separate resolution.
    let missing = beweft::publications::retry(State(h.state.clone()), h.auth.clone(), Path(publication)).await;
    assert_eq!(missing.err().unwrap().0, StatusCode::CONFLICT);
    assert_eq!(sqlx::query_scalar::<_, String>("SELECT state FROM publications WHERE id=$1").bind(publication).fetch_one(&h.state.db).await.unwrap(), "failed");
    sqlx::query("UPDATE publication_attempts SET state='failed' WHERE id=$1").bind(attempt).execute(&h.state.db).await.unwrap();

    // The encrypted checkpoint's attempt must match the checkpoint row too.
    sqlx::query("UPDATE provider_delivery_checkpoints SET attempt_id=$2 WHERE publication_id=$1").bind(publication).bind(Uuid::new_v4()).execute(&h.state.db).await.unwrap();
    let stale = beweft::publications::retry(State(h.state.clone()), h.auth.clone(), Path(publication)).await;
    assert_eq!(stale.err().unwrap().0, StatusCode::CONFLICT);
    sqlx::query("UPDATE provider_delivery_checkpoints SET attempt_id=$2 WHERE publication_id=$1").bind(publication).bind(attempt).execute(&h.state.db).await.unwrap();

    let response = beweft::publications::retry(State(h.state.clone()), h.auth.clone(), Path(publication)).await.unwrap();
    assert_eq!(response.0["data"]["state"], "queued");
    let stored: String = sqlx::query_scalar("SELECT encrypted_checkpoint FROM provider_delivery_checkpoints WHERE publication_id=$1").bind(publication).fetch_one(&h.state.db).await.unwrap();
    let resumed: Value = serde_json::from_str(&crypto::decrypt(&[13; 32], publication, &stored).unwrap()).unwrap();
    assert_eq!(resumed["stage"], "bsky_video_preflight");
    for key in ["rkey", "created_at", "blobs", "attempt_id", "checkpoint_id", "pds_audience"] {
        assert_eq!(resumed[key], original[key], "retry changed {key}");
    }
    assert_eq!(sqlx::query_scalar::<_, Value>("SELECT payload FROM publications WHERE id=$1").bind(publication).fetch_one(&h.state.db).await.unwrap(), payload);
    println!("PASS Bluesky quota retry keeps the original checkpoint identity and rejects stale senders");
}
