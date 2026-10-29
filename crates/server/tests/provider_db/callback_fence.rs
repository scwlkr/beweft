async fn callback_exchange_respects_removal_boundary(h: &Harness) {
    use axum::routing::{get, post};
    use std::time::Duration;

    h.app("youtube", "callback-client", None).await;
    let brand = Uuid::new_v4();
    let channel = Uuid::new_v4();
    sqlx::query("INSERT INTO brands(id,name) VALUES($1,'Callback fixture')")
        .bind(brand)
        .execute(&h.state.db)
        .await
        .unwrap();
    let grant = Channel {
        app: connections::app_config(&h.state, "youtube").await.unwrap(),
        identity: Identity {
            id: "destination".into(),
            name: "Fixture".into(),
            username: None,
        },
        tokens: Tokens {
            access_token: "old".into(),
            refresh_token: None,
            expires_at: None,
            scopes: vec![],
            scopes_known: false,
        },
    };
    let encrypted =
        crypto::encrypt(&[13; 32], channel, &serde_json::to_string(&grant).unwrap()).unwrap();
    sqlx::query("INSERT INTO channels(id,brand_id,name,provider,encrypted_credentials,connection_identity,connection_status) VALUES($1,$2,'Fixture','youtube',$3,$4,'connected')")
        .bind(channel).bind(brand).bind(encrypted).bind(grant.destination_key()).execute(&h.state.db).await.unwrap();
    let router = Router::new()
        .route("/start/{provider}", post(connections::start))
        .route("/callback/{provider}", get(connections::callback))
        .route("/destinations/{id}", get(connections::flow_destinations))
        .route("/complete/{id}", post(connections::complete))
        .route("/disconnect/{id}", post(connections::disconnect))
        .route("/ack/{provider}", post(connections::acknowledge_revocation))
        .with_state(h.state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let session = format!("beweft_session={}", h.session);

    async fn start(
        client: &reqwest::Client,
        base: &str,
        session: &str,
        brand: Uuid,
        channel: Uuid,
    ) -> (String, String, String) {
        let response = client
            .post(format!("{base}/start/youtube"))
            .header(header::COOKIE, session)
            .header("x-beweft-request", "1")
            .json(&json!({"brand_id":brand,"reconnect_channel_id":channel}))
            .send()
            .await
            .unwrap();
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
        let value: Value = response.json().await.unwrap();
        let url = url::Url::parse(value["data"]["authorization_url"].as_str().unwrap()).unwrap();
        let state = url
            .query_pairs()
            .find(|(key, _)| key == "state")
            .unwrap()
            .1
            .to_string();
        (
            value["data"]["flow_id"].as_str().unwrap().to_owned(),
            state,
            cookie,
        )
    }
    async fn assert_uncertain(response: reqwest::Response, secrets: &[&str]) {
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response.text().await.unwrap();
        for secret in secrets {
            assert!(!body.contains(secret), "error response exposed a secret");
        }
        let value: Value = serde_json::from_str(&body).unwrap();
        let message = value["error"]["message"].as_str().unwrap();
        assert!(message.contains("in progress"));
        assert!(message.contains("outcome is unknown if interrupted"));
        assert!(message.contains("access may already have been granted"));
        assert!(message.contains("First inspect your connected channels"));
        assert!(message.contains("If it did not connect"));
        assert!(message.contains("remove only orphaned access"));
        assert!(message.contains("before starting a new authorization"));
    }
    async fn assert_missing_recovery(response: reqwest::Response, secrets: &[&str]) {
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response.text().await.unwrap();
        for secret in secrets {
            assert!(!body.contains(secret), "error response exposed a secret");
        }
        let value: Value = serde_json::from_str(&body).unwrap();
        let message = value["error"]["message"].as_str().unwrap();
        assert!(message.contains("First inspect your connected channels"));
        assert!(message.contains("If it did not connect"));
        assert!(message.contains("remove only orphaned access"));
        assert!(message.contains("before starting a new authorization"));
    }
    let callback = |state: &str, cookie: &str| {
        client
            .get(format!("{base}/callback/youtube"))
            .header(header::COOKIE, cookie)
            .query(&[("state", state), ("code", "fixture-code")])
    };
    let disconnect = || {
        client
            .post(format!("{base}/disconnect/{channel}"))
            .header(header::COOKIE, &session)
            .header("x-beweft-request", "1")
    };
    let ack = || {
        client
            .post(format!("{base}/ack/youtube"))
            .header(header::COOKIE, &session)
            .header("x-beweft-request", "1")
            .json(&json!({"access_removed":true}))
    };
    let complete = |flow: &str| {
        client
            .post(format!("{base}/complete/{flow}"))
            .header(header::COOKIE, &session)
            .header("x-beweft-request", "1")
            .json(&json!({"destination_id":"destination"}))
    };
    let destinations = |flow: &str| {
        client
            .get(format!("{base}/destinations/{flow}"))
            .header(header::COOKIE, &session)
    };

    let (pending_flow, pending_state, pending_cookie) =
        start(&client, &base, &session, brand, channel).await;
    for response in [
        destinations(&pending_flow).send().await.unwrap(),
        complete(&pending_flow).send().await.unwrap(),
    ] {
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let value: Value = response.json().await.unwrap();
        let message = value["error"]["message"].as_str().unwrap();
        assert!(message.contains("has not completed"));
        assert!(!message.contains("remove"));
    }
    let (_, after_state, after_cookie) = start(&client, &base, &session, brand, channel).await;
    let (_, busy_state, busy_cookie) = start(&client, &base, &session, brand, channel).await;
    let (active_flow, active_state, active_cookie) =
        start(&client, &base, &session, brand, channel).await;
    h.provider.exchange_block.store(1, Ordering::SeqCst);
    h.provider.identity_block.store(1, Ordering::SeqCst);
    let before = h.provider.exchange_requests.load(Ordering::SeqCst);
    let active = tokio::spawn(callback(&active_state, &active_cookie).send());
    tokio::time::timeout(
        Duration::from_secs(3),
        h.provider.exchange_started.notified(),
    )
    .await
    .unwrap();
    let active_secrets = [
        "fixture-code",
        "rotated",
        "rotated-refresh",
        &h.session,
        &active_state,
        active_cookie.split_once('=').unwrap().1,
    ];
    assert_uncertain(
        destinations(&active_flow).send().await.unwrap(),
        &active_secrets,
    )
    .await;
    assert_uncertain(
        complete(&active_flow).send().await.unwrap(),
        &active_secrets,
    )
    .await;
    assert_uncertain(
        callback(&active_state, &active_cookie)
            .send()
            .await
            .unwrap(),
        &active_secrets,
    )
    .await;
    let busy = callback(&busy_state, &busy_cookie).send().await.unwrap();
    assert_eq!(busy.status(), StatusCode::CONFLICT);
    let body: Value = busy.json().await.unwrap();
    let message = body["error"]["message"].as_str().unwrap();
    assert!(message.contains("consumed") && message.contains("starting a new authorization"));
    assert!(message.contains("First inspect your connected channels"));
    assert_eq!(
        callback(&busy_state, &busy_cookie)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        h.provider.exchange_requests.load(Ordering::SeqCst),
        before + 1
    );
    assert_eq!(
        disconnect().send().await.unwrap().status(),
        StatusCode::CONFLICT
    );
    assert_eq!(ack().send().await.unwrap().status(), StatusCode::CONFLICT);
    h.provider.exchange_release.notify_one();
    tokio::time::timeout(
        Duration::from_secs(3),
        h.provider.identity_started.notified(),
    )
    .await
    .unwrap();
    assert_eq!(
        disconnect().send().await.unwrap().status(),
        StatusCode::CONFLICT
    );
    assert_eq!(ack().send().await.unwrap().status(), StatusCode::CONFLICT);
    h.provider.identity_release.notify_one();
    assert_eq!(
        active.await.unwrap().unwrap().status(),
        StatusCode::SEE_OTHER
    );
    h.provider.exchange_block.store(0, Ordering::SeqCst);
    h.provider.identity_block.store(0, Ordering::SeqCst);
    let stored: bool = sqlx::query_scalar(
        "SELECT encrypted_candidates IS NOT NULL FROM connection_flows WHERE id=$1",
    )
    .bind(Uuid::parse_str(&active_flow).unwrap())
    .fetch_one(&h.state.db)
    .await
    .unwrap();
    assert!(stored);
    assert_eq!(disconnect().send().await.unwrap().status(), StatusCode::OK);
    assert_eq!(
        callback(&pending_state, &pending_cookie)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        h.provider.exchange_requests.load(Ordering::SeqCst),
        before + 1
    );
    assert_eq!(ack().send().await.unwrap().status(), StatusCode::OK);
    assert_eq!(
        callback(&after_state, &after_cookie)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        h.provider.exchange_requests.load(Ordering::SeqCst),
        before + 1
    );
    assert_eq!(
        complete(&active_flow).send().await.unwrap().status(),
        StatusCode::CONFLICT
    );
    let (fresh, fresh_state, fresh_cookie) = start(&client, &base, &session, brand, channel).await;
    assert_eq!(
        callback(&fresh_state, &fresh_cookie)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::SEE_OTHER
    );
    assert_eq!(
        complete(&fresh).send().await.unwrap().status(),
        StatusCode::OK
    );
    for response in [
        callback(&fresh_state, &fresh_cookie).send().await.unwrap(),
        destinations(&fresh).send().await.unwrap(),
        complete(&fresh).send().await.unwrap(),
    ] {
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response.text().await.unwrap();
        for secret in [
            "fixture-code",
            "rotated",
            "rotated-refresh",
            &h.session,
            &fresh_state,
            fresh_cookie.split_once('=').unwrap().1,
        ] {
            assert!(!body.contains(secret), "error response exposed a secret");
        }
        let value: Value = serde_json::from_str(&body).unwrap();
        let message = value["error"]["message"].as_str().unwrap();
        assert!(message.contains("already completed"));
        assert!(!message.contains("remove"));
    }
    let wrong_correlation = callback(&fresh_state, "beweft_oauth=wrong-correlation")
        .send()
        .await
        .unwrap();
    assert_eq!(wrong_correlation.status(), StatusCode::BAD_REQUEST);
    let value: Value = wrong_correlation.json().await.unwrap();
    assert!(
        !value["error"]["message"]
            .as_str()
            .unwrap()
            .contains("already completed")
    );
    // The completion response may be lost. Even after expiry its durable row
    // still says completed; once cleanup removes it, guide the owner to inspect
    // connected channels before considering orphaned access.
    sqlx::query("UPDATE connection_flows SET expires_at=now()-interval '1 second' WHERE id=$1")
        .bind(Uuid::parse_str(&fresh).unwrap())
        .execute(&h.state.db)
        .await
        .unwrap();
    for response in [
        callback(&fresh_state, &fresh_cookie).send().await.unwrap(),
        destinations(&fresh).send().await.unwrap(),
        complete(&fresh).send().await.unwrap(),
    ] {
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let value: Value = response.json().await.unwrap();
        let message = value["error"]["message"].as_str().unwrap();
        assert!(message.contains("already completed"));
        assert!(!message.contains("remove"));
    }
    let (candidate_flow, candidate_state, candidate_cookie) =
        start(&client, &base, &session, brand, channel).await;
    let completed_secrets = [
        "fixture-code",
        "rotated",
        "rotated-refresh",
        &h.session,
        &fresh_state,
        fresh_cookie.split_once('=').unwrap().1,
    ];
    for response in [
        callback(&fresh_state, &fresh_cookie).send().await.unwrap(),
        destinations(&fresh).send().await.unwrap(),
        complete(&fresh).send().await.unwrap(),
    ] {
        assert_missing_recovery(response, &completed_secrets).await;
    }
    let connected: bool =
        sqlx::query_scalar("SELECT connection_status='connected' FROM channels WHERE id=$1")
            .bind(channel)
            .fetch_one(&h.state.db)
            .await
            .unwrap();
    assert!(connected);
    assert_eq!(
        callback(&candidate_state, &candidate_cookie)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::SEE_OTHER
    );
    let returned = destinations(&candidate_flow).send().await.unwrap();
    assert_eq!(returned.status(), StatusCode::OK);
    let returned: Value = returned.json().await.unwrap();
    assert_eq!(returned["data"]["destinations"][0]["id"], "destination");
    let replay = callback(&candidate_state, &candidate_cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(replay.status(), StatusCode::BAD_REQUEST);
    let replay: Value = replay.json().await.unwrap();
    assert!(
        replay["error"]["message"]
            .as_str()
            .unwrap()
            .contains("continue connection setup")
    );
    sqlx::query("UPDATE connection_flows SET expires_at=now()-interval '1 second' WHERE id=$1")
        .bind(Uuid::parse_str(&candidate_flow).unwrap())
        .execute(&h.state.db)
        .await
        .unwrap();
    let candidate_secrets = [
        "fixture-code",
        "rotated",
        "rotated-refresh",
        &h.session,
        &candidate_state,
        candidate_cookie.split_once('=').unwrap().1,
    ];
    for response in [
        callback(&candidate_state, &candidate_cookie)
            .send()
            .await
            .unwrap(),
        destinations(&candidate_flow).send().await.unwrap(),
        complete(&candidate_flow).send().await.unwrap(),
    ] {
        assert_missing_recovery(response, &candidate_secrets).await;
    }
    let (legacy, legacy_state, legacy_cookie) =
        start(&client, &base, &session, brand, channel).await;
    for response in [
        callback(&candidate_state, &candidate_cookie)
            .send()
            .await
            .unwrap(),
        destinations(&candidate_flow).send().await.unwrap(),
        complete(&candidate_flow).send().await.unwrap(),
    ] {
        assert_missing_recovery(response, &candidate_secrets).await;
    }
    sqlx::query("UPDATE connection_flows SET oauth_generation=NULL WHERE id=$1")
        .bind(Uuid::parse_str(&legacy).unwrap())
        .execute(&h.state.db)
        .await
        .unwrap();
    let exchanges = h.provider.exchange_requests.load(Ordering::SeqCst);
    assert_eq!(
        callback(&legacy_state, &legacy_cookie)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    let (expired, expired_state, expired_cookie) =
        start(&client, &base, &session, brand, channel).await;
    sqlx::query("UPDATE connection_flows SET expires_at=now()-interval '1 second' WHERE id=$1")
        .bind(Uuid::parse_str(&expired).unwrap())
        .execute(&h.state.db)
        .await
        .unwrap();
    let expired_secrets = [
        "fixture-code",
        "rotated",
        "rotated-refresh",
        &h.session,
        &expired_state,
        expired_cookie.split_once('=').unwrap().1,
    ];
    for response in [
        callback(&expired_state, &expired_cookie)
            .send()
            .await
            .unwrap(),
        destinations(&expired).send().await.unwrap(),
        complete(&expired).send().await.unwrap(),
    ] {
        assert_missing_recovery(response, &expired_secrets).await;
    }
    assert_eq!(
        h.provider.exchange_requests.load(Ordering::SeqCst),
        exchanges
    );

    // A different provider's start must not clean up a callback that expires
    // during its exchange, and an expired callback must not redirect to a
    // nonexistent destination picker.
    h.app("x", "other-provider", None).await;
    let (near_expiry, near_state, near_cookie) =
        start(&client, &base, &session, brand, channel).await;
    for response in [
        callback(&expired_state, &expired_cookie)
            .send()
            .await
            .unwrap(),
        destinations(&expired).send().await.unwrap(),
        complete(&expired).send().await.unwrap(),
    ] {
        assert_missing_recovery(response, &expired_secrets).await;
    }
    h.provider.exchange_block.store(1, Ordering::SeqCst);
    let near_callback = tokio::spawn(callback(&near_state, &near_cookie).send());
    tokio::time::timeout(
        Duration::from_secs(3),
        h.provider.exchange_started.notified(),
    )
    .await
    .unwrap();
    sqlx::query(
        "UPDATE connection_flows SET expires_at=clock_timestamp()-interval '1 second' WHERE id=$1",
    )
    .bind(Uuid::parse_str(&near_expiry).unwrap())
    .execute(&h.state.db)
    .await
    .unwrap();
    let other_start = client
        .post(format!("{base}/start/x"))
        .header(header::COOKIE, &session)
        .header("x-beweft-request", "1")
        .json(&json!({"brand_id":brand}))
        .send()
        .await
        .unwrap();
    assert_eq!(other_start.status(), StatusCode::OK);
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM connection_flows WHERE id=$1)")
            .bind(Uuid::parse_str(&near_expiry).unwrap())
            .fetch_one(&h.state.db)
            .await
            .unwrap();
    assert!(
        exists,
        "another provider must not delete an active callback"
    );
    let near_secrets = [
        "fixture-code",
        "rotated",
        &h.session,
        &near_state,
        near_cookie.split_once('=').unwrap().1,
    ];
    for response in [
        callback(&near_state, &near_cookie).send().await.unwrap(),
        destinations(&near_expiry).send().await.unwrap(),
        complete(&near_expiry).send().await.unwrap(),
    ] {
        assert_uncertain(response, &near_secrets).await;
    }
    h.provider.exchange_block.store(0, Ordering::SeqCst);
    h.provider.exchange_release.notify_one();
    let response = near_callback.await.unwrap().unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(response.headers().get(header::LOCATION).is_none());
    let value: Value = response.json().await.unwrap();
    let message = value["error"]["message"].as_str().unwrap();
    assert!(message.contains("First inspect your connected channels"));
    assert!(message.contains("remove only orphaned access"));
    let stored: bool = sqlx::query_scalar(
        "SELECT encrypted_candidates IS NOT NULL FROM connection_flows WHERE id=$1",
    )
    .bind(Uuid::parse_str(&near_expiry).unwrap())
    .fetch_one(&h.state.db)
    .await
    .unwrap();
    assert!(!stored);
    for response in [
        callback(&near_state, &near_cookie).send().await.unwrap(),
        destinations(&near_expiry).send().await.unwrap(),
        complete(&near_expiry).send().await.unwrap(),
    ] {
        assert_uncertain(response, &near_secrets).await;
    }

    // Even if a flow is removed by another database actor during the exchange,
    // a zero-row candidate update cannot produce a success redirect.
    let (removed, removed_state, removed_cookie) =
        start(&client, &base, &session, brand, channel).await;
    for response in [
        callback(&near_state, &near_cookie).send().await.unwrap(),
        destinations(&near_expiry).send().await.unwrap(),
        complete(&near_expiry).send().await.unwrap(),
    ] {
        assert_missing_recovery(response, &near_secrets).await;
    }
    h.provider.exchange_block.store(1, Ordering::SeqCst);
    let removed_callback = tokio::spawn(callback(&removed_state, &removed_cookie).send());
    tokio::time::timeout(
        Duration::from_secs(3),
        h.provider.exchange_started.notified(),
    )
    .await
    .unwrap();
    sqlx::query("DELETE FROM connection_flows WHERE id=$1")
        .bind(Uuid::parse_str(&removed).unwrap())
        .execute(&h.state.db)
        .await
        .unwrap();
    h.provider.exchange_block.store(0, Ordering::SeqCst);
    h.provider.exchange_release.notify_one();
    let response = removed_callback.await.unwrap().unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(response.headers().get(header::LOCATION).is_none());
    let value: Value = response.json().await.unwrap();
    assert!(
        value["error"]["message"]
            .as_str()
            .unwrap()
            .contains("First inspect your connected channels")
    );

    // A session can expire or be revoked while the provider exchange runs.
    // Neither outcome permits a saved candidate or a success redirect.
    for revoked in [false, true] {
        let (flow, state, cookie) = start(&client, &base, &session, brand, channel).await;
        h.provider.exchange_block.store(1, Ordering::SeqCst);
        let request = tokio::spawn(callback(&state, &cookie).send());
        tokio::time::timeout(
            Duration::from_secs(3),
            h.provider.exchange_started.notified(),
        )
        .await
        .unwrap();
        if revoked {
            sqlx::query("DELETE FROM sessions WHERE token_hash=$1")
                .bind(crypto::digest(&h.session))
                .execute(&h.state.db)
                .await
                .unwrap();
        } else {
            sqlx::query("UPDATE sessions SET expires_at=clock_timestamp()-interval '1 second' WHERE token_hash=$1").bind(crypto::digest(&h.session)).execute(&h.state.db).await.unwrap();
        }
        h.provider.exchange_block.store(0, Ordering::SeqCst);
        h.provider.exchange_release.notify_one();
        let response = request.await.unwrap().unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert!(response.headers().get(header::LOCATION).is_none());
        let value: Value = response.json().await.unwrap();
        assert!(
            value["error"]["message"]
                .as_str()
                .unwrap()
                .contains("First inspect your connected channels")
        );
        let stored: bool = sqlx::query_scalar(
            "SELECT encrypted_candidates IS NOT NULL FROM connection_flows WHERE id=$1",
        )
        .bind(Uuid::parse_str(&flow).unwrap())
        .fetch_one(&h.state.db)
        .await
        .unwrap();
        assert!(!stored);
        sqlx::query("INSERT INTO sessions(token_hash,owner_id,expires_at) VALUES($1,$2,now()+interval '1 hour') ON CONFLICT(token_hash) DO UPDATE SET expires_at=EXCLUDED.expires_at")
            .bind(crypto::digest(&h.session)).bind(h.auth.owner_id).execute(&h.state.db).await.unwrap();
    }

    // A canceled callback does not persist a candidate. A remote token may still
    // have been minted, so remove provider access before confirming the boundary.
    let (canceled, state, cookie) = start(&client, &base, &session, brand, channel).await;
    let mut headers = HeaderMap::new();
    headers.insert(header::COOKIE, cookie.parse().unwrap());
    h.provider.exchange_block.store(1, Ordering::SeqCst);
    let app_state = h.state.clone();
    let callback_state = state.clone();
    let canceled_task = tokio::spawn(async move {
        connections::callback(
            State(app_state),
            headers,
            Path("youtube".into()),
            Query(connections::Callback {
                state: Some(callback_state),
                code: Some("fixture-code".into()),
                error: None,
            }),
        )
        .await
    });
    tokio::time::timeout(
        Duration::from_secs(3),
        h.provider.exchange_started.notified(),
    )
    .await
    .unwrap();
    canceled_task.abort();
    assert!(canceled_task.await.unwrap_err().is_cancelled());
    let mut probe = h.state.db.begin().await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        sqlx::query(
            "SELECT provider FROM provider_oauth_fences WHERE provider='youtube' FOR UPDATE",
        )
        .fetch_one(&mut *probe),
    )
    .await
    .unwrap()
    .unwrap();
    let row: (bool, bool) = sqlx::query_as("SELECT consumed_at IS NOT NULL,encrypted_candidates IS NULL FROM connection_flows WHERE id=$1")
        .bind(Uuid::parse_str(&canceled).unwrap()).fetch_one(&mut *probe).await.unwrap();
    assert_eq!(row, (true, true));
    probe.commit().await.unwrap();
    h.provider.exchange_block.store(0, Ordering::SeqCst);
    h.provider.exchange_release.notify_one();
    let canceled_secrets = [
        "fixture-code",
        "rotated",
        "rotated-refresh",
        &h.session,
        &state,
        cookie.split_once('=').unwrap().1,
    ];
    let after_cancel = h.provider.exchange_requests.load(Ordering::SeqCst);
    assert_uncertain(
        destinations(&canceled).send().await.unwrap(),
        &canceled_secrets,
    )
    .await;
    assert_uncertain(complete(&canceled).send().await.unwrap(), &canceled_secrets).await;
    assert_uncertain(
        callback(&state, &cookie).send().await.unwrap(),
        &canceled_secrets,
    )
    .await;
    for response in [
        client
            .get(format!("{base}/callback/youtube"))
            .query(&[("state", state.as_str()), ("code", "fixture-code")])
            .send()
            .await
            .unwrap(),
        client
            .get(format!("{base}/callback/youtube"))
            .header(header::COOKIE, &cookie)
            .query(&[("code", "fixture-code")])
            .send()
            .await
            .unwrap(),
    ] {
        assert_missing_recovery(response, &canceled_secrets).await;
    }
    assert_eq!(
        h.provider.exchange_requests.load(Ordering::SeqCst),
        after_cancel
    );
    // Expiry alone does not turn an interrupted exchange into a known failure.
    sqlx::query("UPDATE connection_flows SET expires_at=now()-interval '1 second' WHERE id=$1")
        .bind(Uuid::parse_str(&canceled).unwrap())
        .execute(&h.state.db)
        .await
        .unwrap();
    assert_uncertain(
        destinations(&canceled).send().await.unwrap(),
        &canceled_secrets,
    )
    .await;
    assert_uncertain(complete(&canceled).send().await.unwrap(), &canceled_secrets).await;
    assert_uncertain(
        callback(&state, &cookie).send().await.unwrap(),
        &canceled_secrets,
    )
    .await;
    assert_eq!(disconnect().send().await.unwrap().status(), StatusCode::OK);
    assert_eq!(ack().send().await.unwrap().status(), StatusCode::OK);
    assert_eq!(
        complete(&canceled).send().await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    // Starting another flow cleans up expired rows; recovery must still be
    // conditional and actionable when the original flow is no longer stored.
    sqlx::query("DELETE FROM connection_flows WHERE id=$1")
        .bind(Uuid::parse_str(&canceled).unwrap())
        .execute(&h.state.db)
        .await
        .unwrap();
    assert_missing_recovery(
        destinations(&canceled).send().await.unwrap(),
        &canceled_secrets,
    )
    .await;
    assert_missing_recovery(complete(&canceled).send().await.unwrap(), &canceled_secrets).await;
    assert_missing_recovery(
        callback(&state, &cookie).send().await.unwrap(),
        &canceled_secrets,
    )
    .await;
    assert_eq!(
        h.provider.exchange_requests.load(Ordering::SeqCst),
        after_cancel
    );
    sqlx::query("DELETE FROM connection_flows WHERE brand_id=$1")
        .bind(brand)
        .execute(&h.state.db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM channels WHERE id=$1")
        .bind(channel)
        .execute(&h.state.db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM brands WHERE id=$1")
        .bind(brand)
        .execute(&h.state.db)
        .await
        .unwrap();
    server.abort();
    println!(
        "PASS blocked exchange/identity, stale callbacks and cancellation obey the manual-removal boundary"
    );
}
