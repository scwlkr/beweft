async fn google_family_and_publication_boundaries(h: &Harness) {
    use beweft::contracts::ReconcileRequest;
    h.app("youtube", "google-youtube-fixture", None).await;
    h.app("google_business", "google-business-fixture", None)
        .await;
    let brand = Uuid::new_v4();
    sqlx::query("INSERT INTO brands(id,name) VALUES($1,'Google fixture')")
        .bind(brand)
        .execute(&h.state.db)
        .await
        .unwrap();
    let mut auth = h.auth.clone();
    auth.brand_ids = vec![brand];
    let google = Uuid::new_v4();
    let youtube = Uuid::new_v4();
    let manual = Uuid::new_v4();
    let google_grant = Channel {
        app: connections::app_config(&h.state, "google_business")
            .await
            .unwrap(),
        identity: Identity {
            parent: Some("accounts/g1".into()),
            id: "locations/l1".into(),
            name: "Fixture".into(),
            username: None,
        },
        tokens: Tokens {
            access_token: "fixture".into(),
            refresh_token: None,
            expires_at: None,
            scopes: vec![],
            scopes_known: false,
        },
    };
    let youtube_grant = Channel {
        app: connections::app_config(&h.state, "youtube").await.unwrap(),
        identity: Identity {
            parent: None,
            id: "youtube-g1".into(),
            name: "Fixture".into(),
            username: None,
        },
        tokens: google_grant.tokens.clone(),
    };
    let google_cipher = crypto::encrypt(
        &[13; 32],
        google,
        &serde_json::to_string(&google_grant).unwrap(),
    )
    .unwrap();
    let youtube_cipher = crypto::encrypt(
        &[13; 32],
        youtube,
        &serde_json::to_string(&youtube_grant).unwrap(),
    )
    .unwrap();
    for (id, provider, cipher, identity) in [
        (google, "google_business", &google_cipher, "locations/l1"),
        (youtube, "youtube", &youtube_cipher, "youtube-g1"),
    ] {
        sqlx::query("INSERT INTO channels(id,brand_id,name,provider,encrypted_credentials,connection_identity,connection_status) VALUES($1,$2,'Fixture',$3,$4,$5,'connected')").bind(id).bind(brand).bind(provider).bind(cipher).bind(identity).execute(&h.state.db).await.unwrap();
    }
    sqlx::query("INSERT INTO channels(id,brand_id,name,provider,target_platform) VALUES($1,$2,'Manual','manual','generic')").bind(manual).bind(brand).execute(&h.state.db).await.unwrap();
    let start = connections::start(
        State(h.state.clone()),
        auth.clone(),
        h.headers(),
        Path("youtube".into()),
        Json(json!({"brand_id":brand})),
    )
    .await
    .unwrap();
    let cookie = start
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let value: Value =
        serde_json::from_slice(&to_bytes(start.into_body(), 100_000).await.unwrap()).unwrap();
    let state = url::Url::parse(value["data"]["authorization_url"].as_str().unwrap())
        .unwrap()
        .query_pairs()
        .find(|(key, _)| key == "state")
        .unwrap()
        .1
        .into_owned();
    let pending_flow_response = connections::start(
        State(h.state.clone()),
        auth.clone(),
        h.headers(),
        Path("google_business".into()),
        Json(json!({"brand_id":brand,"reconnect_channel_id":google})),
    )
    .await
    .unwrap();
    let pending_flow_value: Value = serde_json::from_slice(
        &to_bytes(pending_flow_response.into_body(), 100_000)
            .await
            .unwrap(),
    )
    .unwrap();
    let pending_flow: Uuid = pending_flow_value["data"]["flow_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let candidates = crypto::encrypt(
        &[13; 32],
        pending_flow,
        &serde_json::to_string(&vec![google_grant.clone()]).unwrap(),
    )
    .unwrap();
    sqlx::query(
        "UPDATE connection_flows SET consumed_at=now(),encrypted_candidates=$2 WHERE id=$1",
    )
    .bind(pending_flow)
    .bind(candidates)
    .execute(&h.state.db)
    .await
    .unwrap();
    let post = Uuid::new_v4();
    let youtube_pub = Uuid::new_v4();
    let google_pub = Uuid::new_v4();
    sqlx::query("INSERT INTO posts(id,brand_id,text) VALUES($1,$2,'Fixture')")
        .bind(post)
        .bind(brand)
        .execute(&h.state.db)
        .await
        .unwrap();
    for (id, channel, state) in [
        (youtube_pub, youtube, "sending"),
        (google_pub, google, "queued"),
    ] {
        sqlx::query("INSERT INTO publications(id,post_id,brand_id,channel_id,post_version,scheduled_at,state,payload) VALUES($1,$2,$3,$4,1,now(),$5,$6)")
            .bind(id).bind(post).bind(brand).bind(channel).bind(state).bind(json!({"text":"Fixture","title":"","media_ids":[],"options":{}})).execute(&h.state.db).await.unwrap();
    }
    assert_eq!(
        connections::disconnect(State(h.state.clone()), auth.clone(), Path(google))
            .await
            .err()
            .unwrap()
            .0,
        StatusCode::CONFLICT
    );
    sqlx::query("UPDATE publications SET state='queued' WHERE id=$1")
        .bind(youtube_pub)
        .execute(&h.state.db)
        .await
        .unwrap();
    let inbox = Uuid::new_v4();
    let reply = Uuid::new_v4();
    sqlx::query("INSERT INTO provider_inbox(id,channel_id,provider_item_id,item) VALUES($1,$2,'fixture','{}')").bind(inbox).bind(youtube).execute(&h.state.db).await.unwrap();
    sqlx::query("INSERT INTO provider_replies(id,inbox_id,owner_id,text,state) VALUES($1,$2,$3,'Fixture','sending')").bind(reply).bind(inbox).bind(auth.owner_id).execute(&h.state.db).await.unwrap();
    assert_eq!(
        connections::disconnect(State(h.state.clone()), auth.clone(), Path(google))
            .await
            .err()
            .unwrap()
            .0,
        StatusCode::CONFLICT
    );
    sqlx::query("UPDATE provider_replies SET state='failed' WHERE id=$1")
        .bind(reply)
        .execute(&h.state.db)
        .await
        .unwrap();
    let before: i64 = sqlx::query_scalar(
        "SELECT oauth_generation FROM provider_oauth_fences WHERE provider='youtube'",
    )
    .fetch_one(&h.state.db)
    .await
    .unwrap();
    let result = connections::disconnect(State(h.state.clone()), auth.clone(), Path(google))
        .await
        .unwrap()
        .0;
    let marker: Uuid = result["data"]["revocation_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(result["data"]["provider_revoked"], false);
    let action = result["data"]["next_action"].as_str().unwrap();
    assert!(action.contains("older Google projects") && action.contains("unrelated Google apps"));
    let sibling: (String, String) =
        sqlx::query_as("SELECT connection_status,encrypted_credentials FROM channels WHERE id=$1")
            .bind(youtube)
            .fetch_one(&h.state.db)
            .await
            .unwrap();
    assert_eq!(
        sibling,
        ("reconnect_required".into(), youtube_cipher.clone())
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT state FROM publications WHERE id=$1")
            .bind(youtube_pub)
            .fetch_one(&h.state.db)
            .await
            .unwrap(),
        "queued"
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT state FROM publications WHERE id=$1")
            .bind(google_pub)
            .fetch_one(&h.state.db)
            .await
            .unwrap(),
        "canceled"
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT provider FROM channels WHERE id=$1")
            .bind(manual)
            .fetch_one(&h.state.db)
            .await
            .unwrap(),
        "manual"
    );
    for provider in ["youtube", "google_business"] {
        let status = connections::removal_status(
            State(h.state.clone()),
            auth.clone(),
            Path(provider.into()),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(status["data"]["revocation_id"], json!(marker));
        let save = connections::save_app(
            State(h.state.clone()),
            auth.clone(),
            Path(provider.into()),
            Json(json!({"client_id":"changed","client_secret":"fixture"})),
        )
        .await;
        assert_eq!(save.err().unwrap().0, StatusCode::CONFLICT);
        let fresh = connections::start(
            State(h.state.clone()),
            auth.clone(),
            h.headers(),
            Path(provider.into()),
            Json(json!({"brand_id":brand})),
        )
        .await;
        assert_eq!(fresh.err().unwrap().0, StatusCode::CONFLICT);
    }
    let apps = connections::list_apps(State(h.state.clone()), auth.clone())
        .await
        .unwrap()
        .0;
    for provider in ["youtube", "google_business"] {
        assert_eq!(
            apps["data"]
                .as_array()
                .unwrap()
                .iter()
                .find(|a| a["provider"] == provider)
                .unwrap()["revocation_id"],
            json!(marker)
        );
    }
    assert_eq!(
        connections::acknowledge_revocation(
            State(h.state.clone()),
            auth.clone(),
            Path("pinterest".into()),
            Json(json!({"access_removed":true,"revocation_id":marker}))
        )
        .await
        .err()
        .unwrap()
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        connections::acknowledge_revocation(
            State(h.state.clone()),
            auth.clone(),
            Path("google_business".into()),
            Json(json!({"access_removed":true,"revocation_id":Uuid::new_v4()}))
        )
        .await
        .err()
        .unwrap()
        .0,
        StatusCode::CONFLICT
    );
    let _ = connections::acknowledge_revocation(
        State(h.state.clone()),
        auth.clone(),
        Path("youtube".into()),
        Json(json!({"access_removed":true,"revocation_id":marker})),
    )
    .await
    .unwrap();
    let after: i64 = sqlx::query_scalar(
        "SELECT oauth_generation FROM provider_oauth_fences WHERE provider='youtube'",
    )
    .fetch_one(&h.state.db)
    .await
    .unwrap();
    assert_eq!(after, before + 2);
    assert_eq!(
        connections::complete(
            State(h.state.clone()),
            auth.clone(),
            h.headers(),
            Path(pending_flow),
            Json(json!({"destination_id":"accounts/g1/locations/l1"}))
        )
        .await
        .err()
        .unwrap()
        .0,
        StatusCode::CONFLICT
    );
    // Select a different authenticated parent explicitly for the same stable location.
    let mut reparented = google_grant.clone();
    reparented.identity.parent = Some("accounts/g2".into());
    let new_flow = Uuid::new_v4();
    let candidates = crypto::encrypt(
        &[13; 32],
        new_flow,
        &serde_json::to_string(&vec![reparented]).unwrap(),
    )
    .unwrap();
    sqlx::query("INSERT INTO connection_flows(id,provider,owner_id,brand_id,session_hash,state_hash,correlation_hash,encrypted_verifier,reconnect_channel_id,expires_at,encrypted_candidates,oauth_generation) VALUES($1,'google_business',$2,$3,$4,$5,'fixture','fixture',$6,now()+interval '10 minutes',$7,$8)")
        .bind(new_flow).bind(auth.owner_id).bind(brand).bind(crypto::digest(&h.session)).bind(new_flow.to_string()).bind(google).bind(candidates).bind(after).execute(&h.state.db).await.unwrap();
    assert!(
        connections::complete(
            State(h.state.clone()),
            auth.clone(),
            h.headers(),
            Path(new_flow),
            Json(json!({"destination_id":"accounts/g9/locations/l1"}))
        )
        .await
        .is_err()
    );
    let completed = connections::complete(
        State(h.state.clone()),
        auth.clone(),
        h.headers(),
        Path(new_flow),
        Json(json!({"destination_id":"accounts/g2/locations/l1"})),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(completed["data"]["connection_identity"], "locations/l1");
    let credential: String =
        sqlx::query_scalar("SELECT encrypted_credentials FROM channels WHERE id=$1")
            .bind(google)
            .fetch_one(&h.state.db)
            .await
            .unwrap();
    let restored: Channel =
        serde_json::from_str(&crypto::decrypt(&[13; 32], google, &credential).unwrap()).unwrap();
    assert_eq!(restored.identity.parent.as_deref(), Some("accounts/g2"));
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT state FROM publications WHERE id=$1")
            .bind(youtube_pub)
            .fetch_one(&h.state.db)
            .await
            .unwrap(),
        "queued"
    );
    let exchanges = h.provider.exchange_requests.load(Ordering::SeqCst);
    let mut callback_headers = HeaderMap::new();
    callback_headers.insert(header::COOKIE, cookie.parse().unwrap());
    let stale = connections::callback(
        State(h.state.clone()),
        callback_headers,
        Path("youtube".into()),
        Query(connections::Callback {
            state: Some(state),
            code: Some("fixture".into()),
            error: None,
        }),
    )
    .await;
    assert_eq!(stale.err().unwrap().0, StatusCode::CONFLICT);
    assert_eq!(
        h.provider.exchange_requests.load(Ordering::SeqCst),
        exchanges
    );
    // Restore the target for an isolated worker delivery; sibling queues remain untouched.
    sqlx::query("UPDATE channels SET encrypted_credentials=$2,connection_status='connected',paused=false WHERE id=$1").bind(google).bind(&google_cipher).execute(&h.state.db).await.unwrap();
    sqlx::query("UPDATE publications SET state='queued' WHERE id=$1")
        .bind(google_pub)
        .execute(&h.state.db)
        .await
        .unwrap();
    h.provider.google_mode.store(0, Ordering::SeqCst);
    let writes = h.provider.google_writes.load(Ordering::SeqCst);
    assert!(
        beweft::worker::tick(&h.state, "google-fixture")
            .await
            .unwrap()
    );
    let stored: String = sqlx::query_scalar("SELECT state FROM publications WHERE id=$1")
        .bind(google_pub)
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    assert_eq!(stored, "uncertain");
    assert_eq!(h.provider.google_writes.load(Ordering::SeqCst), writes + 1);
    let status =
        provider_bridge::provider_status(State(h.state.clone()), auth.clone(), Path(google_pub))
            .await
            .unwrap()
            .0;
    assert_eq!(status["data"]["google_delivery"]["phase"], "processing");
    assert_eq!(
        status["data"]["google_delivery"]["post_name"],
        "accounts/g1/locations/l1/localPosts/p1"
    );
    let pending =
        provider_bridge::reconcile(State(h.state.clone()), auth.clone(), Path(google_pub))
            .await
            .unwrap()
            .0;
    assert_eq!(pending["data"]["state"], "uncertain");
    assert!(
        beweft::publications::cancel(State(h.state.clone()), auth.clone(), Path(google_pub))
            .await
            .is_err()
    );
    h.provider.google_mode.store(4, Ordering::SeqCst);
    assert!(
        provider_bridge::reconcile(State(h.state.clone()), auth.clone(), Path(google_pub))
            .await
            .is_err()
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT state FROM publications WHERE id=$1")
            .bind(google_pub)
            .fetch_one(&h.state.db)
            .await
            .unwrap(),
        "uncertain"
    );
    h.provider.google_mode.store(3, Ordering::SeqCst);
    let rejected =
        provider_bridge::reconcile(State(h.state.clone()), auth.clone(), Path(google_pub))
            .await
            .unwrap()
            .0;
    assert_eq!(rejected["data"]["state"], "failed");
    let status =
        provider_bridge::provider_status(State(h.state.clone()), auth.clone(), Path(google_pub))
            .await
            .unwrap()
            .0;
    assert_eq!(status["data"]["google_delivery"]["phase"], "rejected");
    assert_eq!(status["data"]["google_delivery"]["retry_allowed"], true);
    assert_eq!(h.provider.google_writes.load(Ordering::SeqCst), writes + 1);
    let saved_checkpoint: Uuid = status["data"]["google_delivery"]["checkpoint_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    sqlx::query(
        "UPDATE provider_delivery_checkpoints SET checkpoint_id=$2 WHERE publication_id=$1",
    )
    .bind(google_pub)
    .bind(Uuid::new_v4())
    .execute(&h.state.db)
    .await
    .unwrap();
    assert!(
        beweft::publications::retry(State(h.state.clone()), auth.clone(), Path(google_pub))
            .await
            .is_err()
    );
    sqlx::query(
        "UPDATE provider_delivery_checkpoints SET checkpoint_id=$2 WHERE publication_id=$1",
    )
    .bind(google_pub)
    .bind(saved_checkpoint)
    .execute(&h.state.db)
    .await
    .unwrap();
    let _ = beweft::publications::retry(State(h.state.clone()), auth.clone(), Path(google_pub))
        .await
        .unwrap();
    assert_eq!(h.provider.google_writes.load(Ordering::SeqCst), writes + 1);
    h.provider.google_mode.store(1, Ordering::SeqCst);
    assert!(
        beweft::worker::tick(&h.state, "google-fixture")
            .await
            .unwrap()
    );
    assert_eq!(h.provider.google_writes.load(Ordering::SeqCst), writes + 2);
    let status =
        provider_bridge::provider_status(State(h.state.clone()), auth.clone(), Path(google_pub))
            .await
            .unwrap()
            .0;
    assert_eq!(status["data"]["google_delivery"]["phase"], "create_started");
    assert!(status["data"]["google_delivery"]["post_name"].is_null());
    // Even a mistakenly requeued old public checkpoint cannot make a second
    // create when a new worker claim begins.
    sqlx::query("UPDATE publications SET state='queued' WHERE id=$1")
        .bind(google_pub)
        .execute(&h.state.db)
        .await
        .unwrap();
    assert!(
        beweft::worker::tick(&h.state, "google-fixture")
            .await
            .unwrap()
    );
    assert_eq!(h.provider.google_writes.load(Ordering::SeqCst), writes + 2);
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT state FROM publications WHERE id=$1")
            .bind(google_pub)
            .fetch_one(&h.state.db)
            .await
            .unwrap(),
        "uncertain"
    );
    let delivery = &status["data"]["google_delivery"];
    for invalid in [
        "https://example.invalid/post",
        "accounts/g1/locations/other/localPosts/p1",
        "accounts/g1/locations/l1/localPosts/../p1",
        "accounts/g1/locations/l1/localPosts/%70",
    ] {
        let input: ReconcileRequest = serde_json::from_value(json!({"resolution":"published","note":"Inspected Google","external_id":invalid,"observed_attempt_id":delivery["attempt_id"],"observed_checkpoint_id":delivery["checkpoint_id"],"sender_stopped":true,"provider_inspected":true})).unwrap();
        assert!(
            beweft::publications::reconcile(
                State(h.state.clone()),
                auth.clone(),
                Path(google_pub),
                Json(input)
            )
            .await
            .is_err(),
            "{invalid}"
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT state FROM publications WHERE id=$1")
                .bind(google_pub)
                .fetch_one(&h.state.db)
                .await
                .unwrap(),
            "uncertain"
        );
    }
    // A canonical owner-supplied resource for the same location is retained
    // as an attestation, without presenting it as provider verification.
    let owner_post = Uuid::new_v4();
    let owner_pub = Uuid::new_v4();
    let owner_attempt = Uuid::new_v4();
    let owner_checkpoint = Uuid::new_v4();
    sqlx::query("INSERT INTO posts(id,brand_id,text) VALUES($1,$2,'Owner observation')")
        .bind(owner_post)
        .bind(brand)
        .execute(&h.state.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO publications(id,post_id,brand_id,channel_id,post_version,scheduled_at,state,payload) VALUES($1,$2,$3,$4,1,now(),'uncertain',$5)")
        .bind(owner_pub).bind(owner_post).bind(brand).bind(google).bind(json!({"text":"Owner observation","title":"","media_ids":[],"options":{}})).execute(&h.state.db).await.unwrap();
    sqlx::query("INSERT INTO publication_attempts(id,publication_id,state,provider) VALUES($1,$2,'uncertain','google_business')")
        .bind(owner_attempt).bind(owner_pub).execute(&h.state.db).await.unwrap();
    let saved = json!({"stage":"google_post_started","parent":"accounts/g1/locations/l1","attempt_id":owner_attempt,"checkpoint_id":owner_checkpoint});
    let encrypted = crypto::encrypt(&[13; 32], owner_pub, &saved.to_string()).unwrap();
    sqlx::query("INSERT INTO provider_delivery_checkpoints(publication_id,encrypted_checkpoint,attempt_id,checkpoint_id) VALUES($1,$2,$3,$4)")
        .bind(owner_pub).bind(encrypted).bind(owner_attempt).bind(owner_checkpoint).execute(&h.state.db).await.unwrap();
    let owner_input: ReconcileRequest=serde_json::from_value(json!({"resolution":"published","note":"Inspected Google","external_id":"accounts/g2/locations/l1/localPosts/p1","observed_attempt_id":owner_attempt,"observed_checkpoint_id":owner_checkpoint,"sender_stopped":true,"provider_inspected":true})).unwrap();
    let owner_result = beweft::publications::reconcile(
        State(h.state.clone()),
        auth.clone(),
        Path(owner_pub),
        Json(owner_input),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(
        owner_result["data"]["external_id"],
        "accounts/g2/locations/l1/localPosts/p1"
    );
    let owner_receipt: Value = sqlx::query_scalar(
        "SELECT receipt FROM publication_attempts WHERE publication_id=$1 AND state='published'",
    )
    .bind(owner_pub)
    .fetch_one(&h.state.db)
    .await
    .unwrap();
    assert_eq!(owner_receipt["verification"], "owner_attestation");
    let without_id = Uuid::new_v4();
    let without_attempt = Uuid::new_v4();
    let without_checkpoint = Uuid::new_v4();
    sqlx::query("INSERT INTO publications(id,post_id,brand_id,channel_id,post_version,scheduled_at,state,payload) VALUES($1,$2,$3,$4,2,now(),'uncertain',$5)")
        .bind(without_id).bind(owner_post).bind(brand).bind(google).bind(json!({"text":"Owner observation","title":"","media_ids":[],"options":{}})).execute(&h.state.db).await.unwrap();
    sqlx::query("INSERT INTO publication_attempts(id,publication_id,state,provider) VALUES($1,$2,'uncertain','google_business')")
        .bind(without_attempt).bind(without_id).execute(&h.state.db).await.unwrap();
    let saved = json!({"stage":"google_post_started","parent":"accounts/g1/locations/l1","attempt_id":without_attempt,"checkpoint_id":without_checkpoint});
    let encrypted = crypto::encrypt(&[13; 32], without_id, &saved.to_string()).unwrap();
    sqlx::query("INSERT INTO provider_delivery_checkpoints(publication_id,encrypted_checkpoint,attempt_id,checkpoint_id) VALUES($1,$2,$3,$4)")
        .bind(without_id).bind(encrypted).bind(without_attempt).bind(without_checkpoint).execute(&h.state.db).await.unwrap();
    let without_input:ReconcileRequest=serde_json::from_value(json!({"resolution":"published","note":"Inspected Google","external_url":"https://example.invalid/owner","observed_attempt_id":without_attempt,"observed_checkpoint_id":without_checkpoint,"sender_stopped":true,"provider_inspected":true})).unwrap();
    let without_result = beweft::publications::reconcile(
        State(h.state.clone()),
        auth.clone(),
        Path(without_id),
        Json(without_input),
    )
    .await
    .unwrap()
    .0;
    assert!(without_result["data"]["external_id"].is_null());
    assert_eq!(
        without_result["data"]["external_url"],
        "https://example.invalid/owner"
    );
    let reconciliation = |inspected: bool| {
        serde_json::from_value::<ReconcileRequest>(json!({"resolution":"not_published","note":"Remote operation definitively ended","observed_attempt_id":delivery["attempt_id"],"observed_checkpoint_id":delivery["checkpoint_id"],"sender_stopped":true,"provider_inspected":inspected})).unwrap()
    };
    let stale: ReconcileRequest = serde_json::from_value(json!({"resolution":"not_published","note":"Inspection","observed_attempt_id":delivery["attempt_id"],"observed_checkpoint_id":Uuid::new_v4(),"sender_stopped":true,"provider_inspected":true})).unwrap();
    assert!(
        beweft::publications::reconcile(
            State(h.state.clone()),
            auth.clone(),
            Path(google_pub),
            Json(stale)
        )
        .await
        .is_err()
    );
    assert!(
        beweft::publications::reconcile(
            State(h.state.clone()),
            auth.clone(),
            Path(google_pub),
            Json(reconciliation(false))
        )
        .await
        .is_err()
    );
    let _ = beweft::publications::reconcile(
        State(h.state.clone()),
        auth.clone(),
        Path(google_pub),
        Json(reconciliation(true)),
    )
    .await
    .unwrap();
    let status =
        provider_bridge::provider_status(State(h.state.clone()), auth.clone(), Path(google_pub))
            .await
            .unwrap()
            .0;
    assert_eq!(status["data"]["google_delivery"]["retry_allowed"], true);
    let _ = beweft::publications::retry(State(h.state.clone()), auth.clone(), Path(google_pub))
        .await
        .unwrap();
    assert_eq!(h.provider.google_writes.load(Ordering::SeqCst), writes + 2);
    // A public HTTPS capability must be committed before Google fetches the
    // actual bytes during create. The fixture rewrites only to loopback.
    let media = Uuid::new_v4();
    let mut bytes = media_fixture::fixture_bytes(false);
    bytes.resize(11000, 0);
    tokio::fs::write(h.state.config.media_dir.join(media.to_string()), &bytes)
        .await
        .unwrap();
    sqlx::query("INSERT INTO media(id,brand_id,name,mime,size,storage_key,alt_text) VALUES($1,$2,'google.jpg','image/jpeg',$3,$4,'Shared blue photo')")
        .bind(media).bind(brand).bind(bytes.len() as i64).bind(media.to_string()).execute(&h.state.db).await.unwrap();
    let old = &h.state.config;
    let payload =
        json!({"text":"Fixture","title":"Working title","media_ids":[media],"options":{}});
    for origin in [
        "https://localhost:3000",
        "https://127.0.0.1",
        "https://192.168.1.9",
        "https://media.invalid",
    ] {
        let mut test_state = h.state.clone();
        test_state.config = Arc::new(Config {
            public_url: origin.into(),
            secure_cookie: true,
            setup_secret: String::new(),
            media_dir: old.media_dir.clone(),
            fixture_enabled: true,
            max_upload_bytes: old.max_upload_bytes,
            media_quota_bytes: old.media_quota_bytes,
            fixture_delay_ms: 0,
            encryption_key: old.encryption_key,
        });
        let mut tx = h.state.db.begin().await.unwrap();
        assert!(
            provider_bridge::ScheduleFormatValidator::default()
                .validate(&test_state, &mut tx, google, brand, &payload)
                .await
                .is_err(),
            "{origin}"
        );
        tx.rollback().await.unwrap();
    }
    let mut public_state = h.state.clone();
    public_state.config = Arc::new(Config {
        public_url: "https://fixture.google.com".into(),
        secure_cookie: true,
        setup_secret: String::new(),
        media_dir: old.media_dir.clone(),
        fixture_enabled: true,
        max_upload_bytes: old.max_upload_bytes,
        media_quota_bytes: old.media_quota_bytes,
        fixture_delay_ms: 0,
        encryption_key: old.encryption_key,
    });
    let mut tx = h.state.db.begin().await.unwrap();
    provider_bridge::ScheduleFormatValidator::default()
        .validate(&public_state, &mut tx, google, brand, &payload)
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    // Schedule the same asset to Google and X using the real snapshot path.
    // Later library edits must not change either saved delivery description.
    let x = Uuid::new_v4();
    let shared_post = Uuid::new_v4();
    sqlx::query("INSERT INTO channels(id,brand_id,name,provider,connection_status,paused) VALUES($1,$2,'X fixture','x','connected',true)")
        .bind(x).bind(brand).execute(&h.state.db).await.unwrap();
    sqlx::query("INSERT INTO posts(id,brand_id,text,media_ids) VALUES($1,$2,'Shared photo',$3)")
        .bind(shared_post)
        .bind(brand)
        .bind(json!([media]))
        .execute(&h.state.db)
        .await
        .unwrap();
    let schedule: beweft::contracts::SchedulePostRequest=serde_json::from_value(json!({"version":1,"channel_ids":[google,x],"scheduled_at":(chrono::Utc::now()+chrono::Duration::minutes(10)).to_rfc3339_opts(chrono::SecondsFormat::Micros,true)})).unwrap();
    let scheduled = beweft::schedules::schedule_post(
        State(public_state.clone()),
        auth.clone(),
        Path(shared_post),
        Query(Default::default()),
        Json(schedule),
    )
    .await
    .unwrap()
    .0;
    let rows = scheduled["data"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    for row in rows {
        assert_eq!(
            row["payload"]["media_metadata"],
            json!([{"id":media,"alt_text":"Shared blue photo"}])
        );
    }
    let google_photo_pub: Uuid = rows
        .iter()
        .find(|row| row["channel_id"] == json!(google))
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let x_photo_pub: Uuid = rows
        .iter()
        .find(|row| row["channel_id"] == json!(x))
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    sqlx::query(
        "UPDATE media SET alt_text='Edited later',metadata_version=metadata_version+1 WHERE id=$1",
    )
    .bind(media)
    .execute(&h.state.db)
    .await
    .unwrap();
    for id in [google_photo_pub, x_photo_pub] {
        let frozen: Value =
            sqlx::query_scalar("SELECT payload->'media_metadata' FROM publications WHERE id=$1")
                .bind(id)
                .fetch_one(&h.state.db)
                .await
                .unwrap();
        assert_eq!(frozen, json!([{"id":media,"alt_text":"Shared blue photo"}]));
    }
    sqlx::query("UPDATE publications SET state='canceled' WHERE id=$1")
        .bind(google_pub)
        .execute(&h.state.db)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE publications SET scheduled_at=clock_timestamp()-interval '1 second' WHERE id=$1",
    )
    .bind(google_photo_pub)
    .execute(&h.state.db)
    .await
    .unwrap();
    let mut media_state = h.state.clone();
    media_state.config = Arc::new(Config {
        public_url: "https://fixture.google.com".into(),
        secure_cookie: true,
        setup_secret: String::new(),
        media_dir: old.media_dir.clone(),
        fixture_enabled: true,
        max_upload_bytes: old.max_upload_bytes,
        media_quota_bytes: old.media_quota_bytes,
        fixture_delay_ms: 0,
        encryption_key: old.encryption_key,
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    *h.provider.callback_origin.lock().unwrap() =
        format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new()
        .route(
            "/api/v1/provider-media/{id}",
            axum::routing::get(provider_bridge::media_file),
        )
        .with_state(media_state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    h.provider.google_mode.store(5, Ordering::SeqCst);
    assert!(
        beweft::worker::tick(&media_state, "google-media-fixture")
            .await
            .unwrap()
    );
    server.abort();
    let _ = server.await;
    assert_eq!(h.provider.google_writes.load(Ordering::SeqCst), writes + 3);
    assert_eq!(*h.provider.media_received.lock().unwrap(), bytes);
    let body = h.provider.google_body.lock().unwrap().clone();
    assert_eq!(body["media"][0].as_object().unwrap().len(), 2);
    assert_eq!(body["media"][0]["mediaFormat"], "PHOTO");
    assert!(body["media"][0].get("description").is_none());
    let still_frozen: Value =
        sqlx::query_scalar("SELECT payload->'media_metadata' FROM publications WHERE id=$1")
            .bind(google_photo_pub)
            .fetch_one(&h.state.db)
            .await
            .unwrap();
    assert_eq!(
        still_frozen,
        json!([{"id":media,"alt_text":"Shared blue photo"}])
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT state FROM publications WHERE id=$1")
            .bind(google_photo_pub)
            .fetch_one(&h.state.db)
            .await
            .unwrap(),
        "published"
    );
    assert!(sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM provider_media_tokens WHERE media_id=$1 AND expires_at>clock_timestamp() AND expires_at<clock_timestamp()+interval '3 hours')").bind(media).fetch_one(&h.state.db).await.unwrap());
    // Remove this isolated fixture brand and all rows created above.
    sqlx::query("DELETE FROM connection_flows WHERE brand_id=$1")
        .bind(brand)
        .execute(&h.state.db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM provider_replies WHERE inbox_id=$1")
        .bind(inbox)
        .execute(&h.state.db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM provider_inbox WHERE id=$1")
        .bind(inbox)
        .execute(&h.state.db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM provider_delivery_checkpoints WHERE publication_id IN (SELECT id FROM publications WHERE brand_id=$1)")
        .bind(brand)
        .execute(&h.state.db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM publication_attempts WHERE publication_id IN (SELECT id FROM publications WHERE brand_id=$1)")
        .bind(brand)
        .execute(&h.state.db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM publications WHERE brand_id=$1")
        .bind(brand)
        .execute(&h.state.db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM posts WHERE brand_id=$1")
        .bind(brand)
        .execute(&h.state.db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM media WHERE id=$1")
        .bind(media)
        .execute(&h.state.db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM channels WHERE brand_id=$1")
        .bind(brand)
        .execute(&h.state.db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM brands WHERE id=$1")
        .bind(brand)
        .execute(&h.state.db)
        .await
        .unwrap();
    h.provider.google_mode.store(0, Ordering::SeqCst);
    println!(
        "PASS Google family generations, cross-provider removal and checkpointed publication recovery"
    );
}
