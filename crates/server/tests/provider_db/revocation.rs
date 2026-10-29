async fn oauth_revocation_fences_reconnects(h: &Harness) {
    use axum::routing::{get, post};
    use std::time::Duration;

    // Use a separate brand so this test does not alter the other fixture channels.
    let brand = Uuid::new_v4();
    sqlx::query("INSERT INTO brands(id,name) VALUES($1,'Revocation fixture')")
        .bind(brand)
        .execute(&h.state.db)
        .await
        .unwrap();
    h.app("youtube", "revocation-client", None).await;
    let channel = Uuid::new_v4();
    let credentials = Channel {
        app: connections::app_config(&h.state, "youtube").await.unwrap(),
        identity: Identity {
            id: "destination".into(),
            name: "Fixture".into(),
            username: None,
        },
        tokens: Tokens {
            access_token: "old-grant".into(),
            refresh_token: None,
            expires_at: None,
            scopes: vec![],
            scopes_known: false,
        },
    };
    assert!(
        !connections::adapter_for(&credentials.app)
            .await
            .unwrap()
            .revoke(&credentials)
            .await
            .unwrap()
    );
    assert_eq!(h.provider.revoke_requests.load(Ordering::SeqCst), 0);
    let encrypted = crypto::encrypt(
        &[13; 32],
        channel,
        &serde_json::to_string(&credentials).unwrap(),
    )
    .unwrap();
    sqlx::query("INSERT INTO channels(id,brand_id,name,provider,encrypted_credentials,connection_identity,connection_status) VALUES($1,$2,'Fixture','youtube',$3,$4,'connected')")
        .bind(channel).bind(brand).bind(encrypted).bind(credentials.destination_key())
        .execute(&h.state.db).await.unwrap();
    let other_brand = Uuid::new_v4();
    let other_channel = Uuid::new_v4();
    sqlx::query("INSERT INTO brands(id,name) VALUES($1,'Second revocation fixture')")
        .bind(other_brand)
        .execute(&h.state.db)
        .await
        .unwrap();
    let other_encrypted = crypto::encrypt(
        &[13; 32],
        other_channel,
        &serde_json::to_string(&credentials).unwrap(),
    )
    .unwrap();
    sqlx::query("INSERT INTO channels(id,brand_id,name,provider,encrypted_credentials,connection_identity,connection_status) VALUES($1,$2,'Other fixture','youtube',$3,$4,'connected')")
        .bind(other_channel).bind(other_brand).bind(&other_encrypted).bind(credentials.destination_key())
        .execute(&h.state.db).await.unwrap();
    let post_id = Uuid::new_v4();
    let publication = Uuid::new_v4();
    sqlx::query("INSERT INTO posts(id,brand_id,text) VALUES($1,$2,'fixture')")
        .bind(post_id)
        .bind(other_brand)
        .execute(&h.state.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO publications(id,post_id,brand_id,channel_id,post_version,scheduled_at,state,payload) VALUES($1,$2,$3,$4,1,now(),'sending','{}')")
        .bind(publication).bind(post_id).bind(other_brand).bind(other_channel).execute(&h.state.db).await.unwrap();

    // Exercise the handlers and gate ordering through a focused HTTP harness;
    // the production router and separate worker have end-to-end coverage too.
    let api = Router::new()
        .route("/start/{provider}", post(connections::start))
        .route("/callback/{provider}", get(connections::callback))
        .route("/complete/{id}", post(connections::complete))
        .route("/disconnect/{id}", post(connections::disconnect))
        .route("/sync/{id}", post(provider_bridge::sync))
        .route("/ack/{provider}", post(connections::acknowledge_revocation))
        .with_state(h.state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, api).await.unwrap() });
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(8))
        .build()
        .unwrap();
    let session = format!("beweft_session={}", h.session);
    let start = |body: Value| {
        client
            .post(format!("{base}/start/youtube"))
            .header(header::COOKIE, &session)
            .header("x-beweft-request", "1")
            .json(&body)
    };
    let complete = |flow: &str| {
        client
            .post(format!("{base}/complete/{flow}"))
            .header(header::COOKIE, &session)
            .header("x-beweft-request", "1")
            .json(&json!({"destination_id":"destination"}))
    };
    let ack = |removed: bool, revocation_id: Uuid| {
        client
            .post(format!("{base}/ack/youtube"))
            .header(header::COOKIE, &session)
            .header("x-beweft-request", "1")
            .json(&json!({"access_removed":removed,"revocation_id":revocation_id}))
    };

    async fn authorize(
        client: &reqwest::Client,
        base: &str,
        response: reqwest::Response,
    ) -> String {
        assert_eq!(response.status(), StatusCode::OK);
        let cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let body: Value = response.json().await.unwrap();
        let flow = body["data"]["flow_id"].as_str().unwrap().to_owned();
        let url = url::Url::parse(body["data"]["authorization_url"].as_str().unwrap()).unwrap();
        let state = url
            .query_pairs()
            .find(|(key, _)| key == "state")
            .unwrap()
            .1
            .to_string();
        let callback = client
            .get(format!("{base}/callback/youtube"))
            .header(header::COOKIE, cookie)
            .query(&[("state", state), ("code", "fixture-code".into())])
            .send()
            .await
            .unwrap();
        assert_eq!(callback.status(), StatusCode::SEE_OTHER);
        flow
    }

    let request = json!({"brand_id":brand,"reconnect_channel_id":channel});
    let stale = authorize(&client, &base, start(request.clone()).send().await.unwrap()).await;
    let new_brand = Uuid::new_v4();
    sqlx::query("INSERT INTO brands(id,name) VALUES($1,'New destination fixture')")
        .bind(new_brand)
        .execute(&h.state.db)
        .await
        .unwrap();
    let stale_new_channel = authorize(
        &client,
        &base,
        start(json!({"brand_id":new_brand})).send().await.unwrap(),
    )
    .await;
    let revoke_count = h.provider.revoke_requests.load(Ordering::SeqCst);
    let blocked = client
        .post(format!("{base}/disconnect/{channel}"))
        .header(header::COOKIE, &session)
        .header("x-beweft-request", "1")
        .send()
        .await
        .unwrap();
    assert_eq!(blocked.status(), StatusCode::CONFLICT);
    assert_eq!(
        h.provider.revoke_requests.load(Ordering::SeqCst),
        revoke_count
    );
    let statuses: Vec<String> =
        sqlx::query_scalar("SELECT connection_status FROM channels WHERE id=ANY($1) ORDER BY id")
            .bind(&[channel, other_channel][..])
            .fetch_all(&h.state.db)
            .await
            .unwrap();
    assert_eq!(statuses, vec!["connected", "connected"]);
    sqlx::query("UPDATE publications SET state='queued' WHERE id=$1")
        .bind(publication)
        .execute(&h.state.db)
        .await
        .unwrap();
    let inbox = Uuid::new_v4();
    let reply = Uuid::new_v4();
    sqlx::query("INSERT INTO provider_inbox(id,channel_id,provider_item_id,item) VALUES($1,$2,'fixture','{}')")
        .bind(inbox).bind(other_channel).execute(&h.state.db).await.unwrap();
    sqlx::query("INSERT INTO provider_replies(id,inbox_id,owner_id,text,state) VALUES($1,$2,$3,'fixture','sending')")
        .bind(reply).bind(inbox).bind(h.auth.owner_id).execute(&h.state.db).await.unwrap();
    let blocked_reply = client
        .post(format!("{base}/disconnect/{channel}"))
        .header(header::COOKIE, &session)
        .header("x-beweft-request", "1")
        .send()
        .await
        .unwrap();
    assert_eq!(blocked_reply.status(), StatusCode::CONFLICT);
    assert_eq!(
        h.provider.revoke_requests.load(Ordering::SeqCst),
        revoke_count
    );
    sqlx::query("UPDATE provider_replies SET state='failed' WHERE id=$1")
        .bind(reply)
        .execute(&h.state.db)
        .await
        .unwrap();

    // The target is unhealthy and its grant is unreadable; this must still fence
    // the provider and mark other channels before the owner removes Google access.
    sqlx::query("UPDATE channels SET connection_status='reconnect_required',encrypted_credentials='unreadable fixture' WHERE id=$1")
        .bind(channel).execute(&h.state.db).await.unwrap();
    // A sync on another YouTube channel must finish before disconnect can commit.
    h.provider.sync_mode.store(1, Ordering::SeqCst);
    let sync = tokio::spawn(
        client
            .post(format!("{base}/sync/{other_channel}"))
            .header(header::COOKIE, &session)
            .header("x-beweft-request", "1")
            .json(&json!({}))
            .send(),
    );
    tokio::time::timeout(Duration::from_secs(3), h.provider.sync_started.notified())
        .await
        .unwrap();
    let began = std::time::Instant::now();
    let timed_out = client
        .post(format!("{base}/disconnect/{channel}"))
        .header(header::COOKIE, &session)
        .header("x-beweft-request", "1")
        .send()
        .await
        .unwrap();
    assert_eq!(timed_out.status(), StatusCode::CONFLICT);
    assert!(began.elapsed() < Duration::from_secs(7));
    assert_eq!(
        h.provider.revoke_requests.load(Ordering::SeqCst),
        revoke_count
    );
    let mut probe = h.state.db.begin().await.unwrap();
    sqlx::query("SELECT provider FROM provider_oauth_fences WHERE provider='youtube' FOR UPDATE")
        .fetch_one(&mut *probe)
        .await
        .unwrap();
    probe.commit().await.unwrap();
    let mut disconnect = tokio::spawn(
        client
            .post(format!("{base}/disconnect/{channel}"))
            .header(header::COOKIE, &session)
            .header("x-beweft-request", "1")
            .send(),
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(150), &mut disconnect)
            .await
            .is_err()
    );
    assert_eq!(
        h.provider.revoke_requests.load(Ordering::SeqCst),
        revoke_count
    );
    h.provider.sync_release.notify_one();
    assert_eq!(sync.await.unwrap().unwrap().status(), StatusCode::OK);
    // Simulate a committed disconnect whose response body is lost to the owner.
    let outcome = disconnect.await.unwrap().unwrap();
    assert_eq!(outcome.status(), StatusCode::OK);
    drop(outcome);
    assert_eq!(
        h.provider.revoke_requests.load(Ordering::SeqCst),
        revoke_count
    );
    // Repeats while manual removal is still pending must leave the same
    // durable boundary in place, even when they arrive concurrently.
    let pending_before: (i64, Option<Uuid>) = sqlx::query_as(
        "SELECT oauth_generation,revocation_id FROM provider_oauth_fences WHERE provider='youtube'",
    )
    .fetch_one(&h.state.db)
    .await
    .unwrap();
    assert!(pending_before.1.is_some());
    let sibling_before: (String, String, bool) = sqlx::query_as(
        "SELECT connection_status,encrypted_credentials,paused FROM channels WHERE id=$1",
    )
    .bind(other_channel)
    .fetch_one(&h.state.db)
    .await
    .unwrap();
    let target_before: (String, Option<String>, bool) = sqlx::query_as(
        "SELECT connection_status,encrypted_credentials,paused FROM channels WHERE id=$1",
    )
    .bind(channel)
    .fetch_one(&h.state.db)
    .await
    .unwrap();
    let queue_before: String = sqlx::query_scalar("SELECT state FROM publications WHERE id=$1")
        .bind(publication)
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    let repeat = || {
        client
            .post(format!("{base}/disconnect/{channel}"))
            .header(header::COOKIE, &session)
            .header("x-beweft-request", "1")
            .send()
    };
    let response = repeat().await.unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = response.text().await.unwrap();
    assert!(!body.contains(&h.session) && !body.contains("rotated-refresh"));
    let value: Value = serde_json::from_str(&body).unwrap();
    let message = value["error"]["message"].as_str().unwrap().to_owned();
    assert!(message.contains("other channels") && message.contains("unrelated accounts"));
    let wait = message.find("Wait for any provider operation").unwrap();
    let inspect = message.find("First inspect your channels").unwrap();
    let remove = message.find("then remove the affected access").unwrap();
    let confirm = message
        .find("Only after removal is complete, confirm")
        .unwrap();
    assert!(wait < inspect && inspect < remove && remove < confirm);

    // If another operation holds the row, report busy and require a current
    // recheck rather than issuing possibly stale manual-removal instructions.
    let mut locked = h.state.db.begin().await.unwrap();
    sqlx::query("SELECT provider FROM provider_oauth_fences WHERE provider='youtube' FOR UPDATE")
        .fetch_one(&mut *locked)
        .await
        .unwrap();
    let response = tokio::time::timeout(Duration::from_secs(2), repeat())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let locked_body: Value = response.json().await.unwrap();
    let locked_message = locked_body["error"]["message"].as_str().unwrap();
    assert!(locked_message.contains("retry and check the current provider status"));
    assert!(!locked_message.contains("connected-app settings"));
    locked.rollback().await.unwrap();
    let (first_repeat, second_repeat) = tokio::join!(repeat(), repeat());
    for response in [first_repeat.unwrap(), second_repeat.unwrap()] {
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body: Value = response.json().await.unwrap();
        let current = body["error"]["message"].as_str().unwrap();
        assert!(
            current == message
                || (current.contains("retry and check the current provider status")
                    && !current.contains("connected-app settings"))
        );
    }
    let pending_after: (i64, Option<Uuid>) = sqlx::query_as(
        "SELECT oauth_generation,revocation_id FROM provider_oauth_fences WHERE provider='youtube'",
    )
    .fetch_one(&h.state.db)
    .await
    .unwrap();
    assert_eq!(pending_after, pending_before);
    let sibling_after: (String, String, bool) = sqlx::query_as(
        "SELECT connection_status,encrypted_credentials,paused FROM channels WHERE id=$1",
    )
    .bind(other_channel)
    .fetch_one(&h.state.db)
    .await
    .unwrap();
    let target_after: (String, Option<String>, bool) = sqlx::query_as(
        "SELECT connection_status,encrypted_credentials,paused FROM channels WHERE id=$1",
    )
    .bind(channel)
    .fetch_one(&h.state.db)
    .await
    .unwrap();
    let queue_after: String = sqlx::query_scalar("SELECT state FROM publications WHERE id=$1")
        .bind(publication)
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    assert_eq!(sibling_after, sibling_before);
    assert_eq!(target_after, target_before);
    assert_eq!(queue_after, queue_before);
    assert_eq!(
        h.provider.revoke_requests.load(Ordering::SeqCst),
        revoke_count
    );
    assert_eq!(
        complete(&stale).send().await.unwrap().status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        complete(&stale_new_channel).send().await.unwrap().status(),
        StatusCode::CONFLICT
    );
    let blocked_start = start(request.clone()).send().await.unwrap();
    assert_eq!(blocked_start.status(), StatusCode::CONFLICT);
    let body: Value = blocked_start.json().await.unwrap();
    assert_eq!(body["error"]["message"], message);
    assert_eq!(
        ack(false, h.revocation_marker("youtube").await).send().await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    let status: (String, bool) = sqlx::query_as(
        "SELECT connection_status,encrypted_credentials IS NULL FROM channels WHERE id=$1",
    )
    .bind(channel)
    .fetch_one(&h.state.db)
    .await
    .unwrap();
    assert_eq!(status, ("disconnected".to_string(), true));
    let other_status: (String, String, bool) = sqlx::query_as(
        "SELECT connection_status,encrypted_credentials,paused FROM channels WHERE id=$1",
    )
    .bind(other_channel)
    .fetch_one(&h.state.db)
    .await
    .unwrap();
    assert_eq!(
        other_status,
        ("reconnect_required".into(), other_encrypted, false)
    );
    let refreshes = h.provider.refreshes.load(Ordering::SeqCst);
    let syncs = h.provider.sync_requests.load(Ordering::SeqCst);
    assert!(
        connections::load_channel(&h.state, other_channel)
            .await
            .is_err()
    );
    assert_eq!(
        client
            .post(format!("{base}/sync/{other_channel}"))
            .header(header::COOKIE, &session)
            .header("x-beweft-request", "1")
            .json(&json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    let manual = Uuid::new_v4();
    let manual_post = Uuid::new_v4();
    let manual_publication = Uuid::new_v4();
    sqlx::query("INSERT INTO channels(id,brand_id,name,provider) VALUES($1,$2,'Manual','manual')")
        .bind(manual)
        .bind(brand)
        .execute(&h.state.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO posts(id,brand_id,text) VALUES($1,$2,'manual')")
        .bind(manual_post)
        .bind(brand)
        .execute(&h.state.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO publications(id,post_id,brand_id,channel_id,post_version,scheduled_at,state,payload) VALUES($1,$2,$3,$4,1,now()-interval '1 minute','queued','{}')")
        .bind(manual_publication).bind(manual_post).bind(brand).bind(manual).execute(&h.state.db).await.unwrap();
    assert!(
        beweft::worker::tick(&h.state, "revocation-test-worker")
            .await
            .unwrap()
    );
    let manual_state: String = sqlx::query_scalar("SELECT state FROM publications WHERE id=$1")
        .bind(manual_publication)
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    assert_eq!(manual_state, "manual_required");
    assert!(
        !beweft::worker::tick(&h.state, "revocation-test-worker")
            .await
            .unwrap()
    );
    let queued: String = sqlx::query_scalar("SELECT state FROM publications WHERE id=$1")
        .bind(publication)
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    assert_eq!(queued, "queued");
    assert_eq!(h.provider.refreshes.load(Ordering::SeqCst), refreshes);
    assert_eq!(h.provider.sync_requests.load(Ordering::SeqCst), syncs);
    assert_eq!(ack(true, h.revocation_marker("youtube").await).send().await.unwrap().status(), StatusCode::OK);
    assert_eq!(
        complete(&stale).send().await.unwrap().status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        complete(&stale_new_channel).send().await.unwrap().status(),
        StatusCode::CONFLICT
    );
    // A was disconnected and removal was confirmed. Connect B again, then
    // repeat A's disconnect: B's fresh grant and queued work must survive.
    let other_request = json!({"brand_id":other_brand,"reconnect_channel_id":other_channel});
    let other_fresh = authorize(&client, &base, start(other_request).send().await.unwrap()).await;
    assert_eq!(
        complete(&other_fresh).send().await.unwrap().status(),
        StatusCode::OK
    );
    let restored: (String, bool) =
        sqlx::query_as("SELECT connection_status,paused FROM channels WHERE id=$1")
            .bind(other_channel)
            .fetch_one(&h.state.db)
            .await
            .unwrap();
    assert_eq!(restored, ("connected".into(), false));
    let before_sibling: (String, String, bool) = sqlx::query_as(
        "SELECT connection_status,encrypted_credentials,paused FROM channels WHERE id=$1",
    )
    .bind(other_channel)
    .fetch_one(&h.state.db)
    .await
    .unwrap();
    let before_fence: (i64, Option<Uuid>) = sqlx::query_as(
        "SELECT oauth_generation,revocation_id FROM provider_oauth_fences WHERE provider='youtube'",
    )
    .fetch_one(&h.state.db)
    .await
    .unwrap();
    assert!(before_fence.1.is_none());
    let repeated = client
        .post(format!("{base}/disconnect/{channel}"))
        .header(header::COOKIE, &session)
        .header("x-beweft-request", "1")
        .send()
        .await
        .unwrap();
    assert_eq!(repeated.status(), StatusCode::OK);
    let repeated: Value = repeated.json().await.unwrap();
    assert_eq!(repeated["data"]["disconnected"], true);
    assert_eq!(repeated["data"]["provider_revoked"], false);
    assert_eq!(repeated["data"]["channels_requiring_reconnection"], 0);
    let message = repeated["data"]["next_action"].as_str().unwrap();
    assert!(message.contains("already disconnected") && message.contains("No new"));
    assert!(!message.contains("confirm") && !message.contains("connected-app"));
    let after_sibling: (String, String, bool) = sqlx::query_as(
        "SELECT connection_status,encrypted_credentials,paused FROM channels WHERE id=$1",
    )
    .bind(other_channel)
    .fetch_one(&h.state.db)
    .await
    .unwrap();
    let after_fence: (i64, Option<Uuid>) = sqlx::query_as(
        "SELECT oauth_generation,revocation_id FROM provider_oauth_fences WHERE provider='youtube'",
    )
    .fetch_one(&h.state.db)
    .await
    .unwrap();
    assert_eq!(after_sibling, before_sibling);
    assert_eq!(after_fence, before_fence);
    let queued: String = sqlx::query_scalar("SELECT state FROM publications WHERE id=$1")
        .bind(publication)
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    assert_eq!(queued, "queued");
    assert_eq!(
        h.provider.revoke_requests.load(Ordering::SeqCst),
        revoke_count
    );

    let fresh = authorize(&client, &base, start(request.clone()).send().await.unwrap()).await;
    assert_eq!(
        complete(&fresh).send().await.unwrap().status(),
        StatusCode::OK
    );
    assert_eq!(
        connections::load_channel(&h.state, channel)
            .await
            .unwrap()
            .tokens
            .access_token,
        "rotated"
    );

    let legacy = authorize(&client, &base, start(request.clone()).send().await.unwrap()).await;
    sqlx::query("UPDATE connection_flows SET oauth_generation=NULL WHERE id=$1")
        .bind(Uuid::parse_str(&legacy).unwrap())
        .execute(&h.state.db)
        .await
        .unwrap();
    assert_eq!(
        complete(&legacy).send().await.unwrap().status(),
        StatusCode::CONFLICT
    );

    // A missing app configuration and absent local ciphertext also cannot
    // establish that Google no longer has the previously authorized grant.
    sqlx::query("DELETE FROM provider_apps WHERE provider='youtube'")
        .execute(&h.state.db)
        .await
        .unwrap();
    sqlx::query("UPDATE channels SET encrypted_credentials=NULL,connection_status='reconnect_required' WHERE id=$1")
        .bind(channel).execute(&h.state.db).await.unwrap();
    let result = client
        .post(format!("{base}/disconnect/{channel}"))
        .header(header::COOKIE, &session)
        .header("x-beweft-request", "1")
        .send()
        .await
        .unwrap();
    assert_eq!(result.status(), StatusCode::OK);
    let pending: Option<Uuid> = sqlx::query_scalar(
        "SELECT revocation_id FROM provider_oauth_fences WHERE provider='youtube'",
    )
    .fetch_one(&h.state.db)
    .await
    .unwrap();
    assert!(pending.is_some());
    let sibling: String = sqlx::query_scalar("SELECT connection_status FROM channels WHERE id=$1")
        .bind(other_channel)
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    assert_eq!(sibling, "reconnect_required");
    h.app("youtube", "revocation-client", None).await;
    assert_eq!(
        start(request.clone()).send().await.unwrap().status(),
        StatusCode::CONFLICT
    );
    assert_eq!(ack(true, h.revocation_marker("youtube").await).send().await.unwrap().status(), StatusCode::OK);
    let recovered = authorize(&client, &base, start(request).send().await.unwrap()).await;
    assert_eq!(
        complete(&recovered).send().await.unwrap().status(),
        StatusCode::OK
    );
    assert_eq!(
        h.provider.revoke_requests.load(Ordering::SeqCst),
        revoke_count
    );
    server.abort();
    h.provider.sync_mode.store(0, Ordering::SeqCst);
    println!(
        "PASS Google manual removal fence, unhealthy/missing grants, and HTTP/PostgreSQL reconnect recovery"
    );
}

async fn populated_oauth_fence_upgrade(h: &Harness) {
    // Run the previous migrations against populated records in a rolled-back schema.
    let schema = format!("oauth_upgrade_{}", Uuid::new_v4().simple());
    let mut tx = h.state.db.begin().await.unwrap();
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(&format!("SET LOCAL search_path TO {schema}"))
        .execute(&mut *tx)
        .await
        .unwrap();
    for migration in [
        include_str!("../../../../migrations/0001_foundation.sql"),
        include_str!("../../../../migrations/0002_connections.sql"),
        include_str!("../../../../migrations/0003_scheduling_mode.sql"),
        include_str!("../../../../migrations/0004_provider_grant_binding.sql"),
        include_str!("../../../../migrations/0005_workflows.sql"),
        include_str!("../../../../migrations/0006_provider_reply_history.sql"),
        include_str!("../../../../migrations/0007_reply_delivery_deadline.sql"),
    ] {
        sqlx::raw_sql(migration).execute(&mut *tx).await.unwrap();
    }
    let owner = Uuid::new_v4();
    let brand = Uuid::new_v4();
    let channel = Uuid::new_v4();
    let flow = Uuid::new_v4();
    let orphan_channel = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO owners(id,email,password_hash) VALUES($1,'upgrade@example.invalid','hash')",
    )
    .bind(owner)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("INSERT INTO brands(id,name) VALUES($1,'Upgrade')")
        .bind(brand)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO provider_apps(id,provider,client_id,encrypted_config) VALUES($1,'youtube','client','config')").bind(Uuid::new_v4()).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO channels(id,brand_id,name,provider,encrypted_credentials,connection_status) VALUES($1,$2,'Existing','youtube','credentials','connected')").bind(channel).bind(brand).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO channels(id,brand_id,name,provider,encrypted_credentials,connection_status) VALUES($1,$2,'Orphan','mastodon','credentials','reconnect_required')").bind(orphan_channel).bind(brand).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO connection_flows(id,provider,owner_id,brand_id,session_hash,state_hash,correlation_hash,encrypted_verifier,encrypted_candidates,expires_at) VALUES($1,'youtube',$2,$3,'session','state','correlation','verifier','candidate',now()+interval '10 minutes')").bind(flow).bind(owner).bind(brand).execute(&mut *tx).await.unwrap();
    sqlx::raw_sql(include_str!(
        "../../../../migrations/0008_oauth_revocation_fence.sql"
    ))
    .execute(&mut *tx)
    .await
    .unwrap();
    let app: (String, i64, Option<Uuid>) = sqlx::query_as("SELECT a.encrypted_config,f.oauth_generation,f.revocation_id FROM provider_apps a JOIN provider_oauth_fences f USING(provider) WHERE a.provider='youtube'").fetch_one(&mut *tx).await.unwrap();
    let old_flow: (String, Option<i64>) = sqlx::query_as(
        "SELECT encrypted_candidates,oauth_generation FROM connection_flows WHERE id=$1",
    )
    .bind(flow)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    let credentials: String =
        sqlx::query_scalar("SELECT encrypted_credentials FROM channels WHERE id=$1")
            .bind(channel)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(app, ("config".into(), 0, None));
    assert_eq!(old_flow, ("candidate".into(), None));
    assert_eq!(credentials, "credentials");
    let orphan_fence: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM provider_oauth_fences WHERE provider='mastodon')",
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert!(
        orphan_fence,
        "a stored grant must be fenced even if its app configuration is missing"
    );
    tx.rollback().await.unwrap();
    println!("PASS populated provider migration preserves grants and fences old flows");
}
