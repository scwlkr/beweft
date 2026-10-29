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

    let (_, pending_state, pending_cookie) = start(&client, &base, &session, brand, channel).await;
    let (_, after_state, after_cookie) = start(&client, &base, &session, brand, channel).await;
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
    let (legacy, legacy_state, legacy_cookie) =
        start(&client, &base, &session, brand, channel).await;
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
    assert_eq!(
        callback(&expired_state, &expired_cookie)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        h.provider.exchange_requests.load(Ordering::SeqCst),
        exchanges
    );

    // A canceled callback does not persist a candidate. A remote token may still
    // have been minted, so remove provider access before confirming the boundary.
    let (canceled, state, cookie) = start(&client, &base, &session, brand, channel).await;
    let mut headers = HeaderMap::new();
    headers.insert(header::COOKIE, cookie.parse().unwrap());
    h.provider.exchange_block.store(1, Ordering::SeqCst);
    let app_state = h.state.clone();
    let canceled_task = tokio::spawn(async move {
        connections::callback(
            State(app_state),
            headers,
            Path("youtube".into()),
            Query(connections::Callback {
                state: Some(state),
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
    assert_eq!(disconnect().send().await.unwrap().status(), StatusCode::OK);
    assert_eq!(ack().send().await.unwrap().status(), StatusCode::OK);
    assert_eq!(
        complete(&canceled).send().await.unwrap().status(),
        StatusCode::BAD_REQUEST
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
