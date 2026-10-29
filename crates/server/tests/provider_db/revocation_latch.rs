async fn provider_gate_orders_manual_removal_and_recovery(h: &Harness) {
    use axum::routing::{get, post};
    use std::time::Duration;

    h.app("mastodon", "latch-client", Some("https://social.example"))
        .await;
    let brand = Uuid::new_v4();
    let ids = [Uuid::new_v4(), Uuid::new_v4()];
    sqlx::query("INSERT INTO brands(id,name) VALUES($1,'Manual removal fixture')")
        .bind(brand)
        .execute(&h.state.db)
        .await
        .unwrap();
    let app = connections::app_config(&h.state, "mastodon").await.unwrap();
    let mut sibling_cipher = String::new();
    for (index, id) in ids.iter().enumerate() {
        let grant = Channel {
            app: app.clone(),
            identity: Identity {
                id: format!("masto-{index}"),
                name: "Fixture".into(),
                username: None,
            },
            tokens: Tokens {
                access_token: "latch-access-fixture".into(),
                refresh_token: None,
                expires_at: None,
                scopes: vec![],
                scopes_known: false,
            },
        };
        let encrypted =
            crypto::encrypt(&[13; 32], *id, &serde_json::to_string(&grant).unwrap()).unwrap();
        if index == 1 {
            sibling_cipher = encrypted.clone();
        }
        sqlx::query("INSERT INTO channels(id,brand_id,name,provider,encrypted_credentials,connection_identity,connection_status) VALUES($1,$2,'Fixture','mastodon',$3,$4,'connected')")
            .bind(id).bind(brand).bind(encrypted).bind(grant.destination_key()).execute(&h.state.db).await.unwrap();
    }
    let post_id = Uuid::new_v4();
    let publication = Uuid::new_v4();
    sqlx::query("INSERT INTO posts(id,brand_id,text) VALUES($1,$2,'Manual removal fixture')")
        .bind(post_id)
        .bind(brand)
        .execute(&h.state.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO publications(id,post_id,brand_id,channel_id,post_version,scheduled_at,state,payload) VALUES($1,$2,$3,$4,1,now(),'queued','{}')")
        .bind(publication).bind(post_id).bind(brand).bind(ids[1]).execute(&h.state.db).await.unwrap();
    let router = Router::new()
        .route("/disconnect/{id}", post(connections::disconnect))
        .route("/start/{provider}", post(connections::start))
        .route("/callback/{provider}", get(connections::callback))
        .route("/destinations/{id}", get(connections::flow_destinations))
        .route("/complete/{id}", post(connections::complete))
        .route("/ack/{provider}", post(connections::acknowledge_revocation))
        .with_state(h.state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(8))
        .build()
        .unwrap();
    let session = format!("beweft_session={}", h.session);
    async fn start(
        client: &reqwest::Client,
        base: &str,
        session: &str,
        brand: Uuid,
    ) -> (String, String, String) {
        let response = client
            .post(format!("{base}/start/mastodon"))
            .header(header::COOKIE, session)
            .header("x-beweft-request", "1")
            .json(&json!({"brand_id":brand}))
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
    async fn assert_busy(response: reqwest::Response, secrets: &[&str]) {
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = response.text().await.unwrap();
        for secret in secrets {
            assert!(!body.contains(secret), "busy response exposed a secret");
        }
        let value: Value = serde_json::from_str(&body).unwrap();
        let message = value["error"]["message"].as_str().unwrap();
        assert!(
            message.contains("Wait for it to finish")
                && message.contains("retry and check the current provider status")
        );
        assert!(
            !message.contains("connected-app settings")
                && !message.contains("remove only orphaned")
        );
    }
    let callback = |state: &str, cookie: &str| {
        client
            .get(format!("{base}/callback/mastodon"))
            .header(header::COOKIE, cookie)
            .query(&[("state", state), ("code", "fixture-code")])
    };
    let complete = |flow: &str, body: Value| {
        client
            .post(format!("{base}/complete/{flow}"))
            .header(header::COOKIE, &session)
            .header("x-beweft-request", "1")
            .json(&body)
    };
    let disconnect = || {
        client
            .post(format!("{base}/disconnect/{}", ids[0]))
            .header(header::COOKIE, &session)
            .header("x-beweft-request", "1")
    };
    let ack = |revocation_id: Uuid| {
        client
            .post(format!("{base}/ack/mastodon"))
            .header(header::COOKIE, &session)
            .header("x-beweft-request", "1")
            .json(&json!({"access_removed":true,"revocation_id":revocation_id}))
    };

    let (candidate, state, cookie) = start(&client, &base, &session, brand).await;
    assert_eq!(
        callback(&state, &cookie).send().await.unwrap().status(),
        StatusCode::SEE_OTHER
    );
    let (active, active_state, active_cookie) = start(&client, &base, &session, brand).await;
    let (_, missing_code_state, missing_code_cookie) = start(&client, &base, &session, brand).await;
    let (invalid_verifier, invalid_state, invalid_cookie) =
        start(&client, &base, &session, brand).await;
    let (_, stale_before_state, stale_before_cookie) = start(&client, &base, &session, brand).await;
    let (_, stale_after_state, stale_after_cookie) = start(&client, &base, &session, brand).await;
    sqlx::query("UPDATE connection_flows SET encrypted_verifier='invalid' WHERE id=$1")
        .bind(Uuid::parse_str(&invalid_verifier).unwrap())
        .execute(&h.state.db)
        .await
        .unwrap();
    let secrets = [
        "latch-access-fixture",
        "fixture-code",
        &h.session,
        &active_state,
        active_cookie.split_once('=').unwrap().1,
    ];
    // Earlier cases may have left a Notify permit after canceling a callback.
    // Drain it, then observe this exchange's counter rather than a stale signal.
    let _ = tokio::time::timeout(
        Duration::from_millis(10),
        h.provider.exchange_release.notified(),
    )
    .await;
    let exchanges_before_block = h.provider.exchange_requests.load(Ordering::SeqCst);
    h.provider.exchange_block.store(1, Ordering::SeqCst);
    let active_request = tokio::spawn(callback(&active_state, &active_cookie).send());
    tokio::time::timeout(Duration::from_secs(3), async {
        while h.provider.exchange_requests.load(Ordering::SeqCst) == exchanges_before_block {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();

    // Every error path that could advise access removal must first enter the
    // provider gate, including validation before candidates or callback code.
    assert_busy(
        client
            .get(format!("{base}/callback/mastodon"))
            .send()
            .await
            .unwrap(),
        &secrets,
    )
    .await;
    assert_busy(
        client
            .get(format!("{base}/callback/mastodon"))
            .query(&[("state", "missing-cookie")])
            .send()
            .await
            .unwrap(),
        &secrets,
    )
    .await;
    assert_busy(
        client
            .get(format!("{base}/callback/mastodon"))
            .header(header::COOKIE, "beweft_oauth=unknown")
            .query(&[("state", "unknown")])
            .send()
            .await
            .unwrap(),
        &secrets,
    )
    .await;
    assert_busy(
        client
            .get(format!("{base}/callback/mastodon"))
            .header(header::COOKIE, &missing_code_cookie)
            .query(&[("state", missing_code_state.as_str())])
            .send()
            .await
            .unwrap(),
        &secrets,
    )
    .await;
    assert_busy(
        callback(&invalid_state, &invalid_cookie)
            .send()
            .await
            .unwrap(),
        &secrets,
    )
    .await;
    assert_busy(
        complete(&candidate, json!({"destination_id":"wrong"}))
            .send()
            .await
            .unwrap(),
        &secrets,
    )
    .await;
    assert_busy(
        complete(
            &candidate,
            json!({"destination_id":"masto-destination","timezone":"invalid"}),
        )
        .send()
        .await
        .unwrap(),
        &secrets,
    )
    .await;
    let candidate_id = Uuid::parse_str(&candidate).unwrap();
    let saved: String =
        sqlx::query_scalar("SELECT encrypted_candidates FROM connection_flows WHERE id=$1")
            .bind(candidate_id)
            .fetch_one(&h.state.db)
            .await
            .unwrap();
    sqlx::query("UPDATE connection_flows SET encrypted_candidates='invalid' WHERE id=$1")
        .bind(candidate_id)
        .execute(&h.state.db)
        .await
        .unwrap();
    assert_busy(
        client
            .get(format!("{base}/destinations/{candidate}"))
            .header(header::COOKIE, &session)
            .send()
            .await
            .unwrap(),
        &secrets,
    )
    .await;
    sqlx::query("UPDATE connection_flows SET encrypted_candidates=$2 WHERE id=$1")
        .bind(candidate_id)
        .bind(saved)
        .execute(&h.state.db)
        .await
        .unwrap();
    assert_busy(disconnect().send().await.unwrap(), &secrets).await;
    // A cleaned-up flow has no provider identity to admit. Its response stays
    // neutral even while an unrelated provider operation is in progress.
    let unknown = client
        .get(format!("{base}/destinations/{}", Uuid::new_v4()))
        .header(header::COOKIE, &session)
        .send()
        .await
        .unwrap();
    assert_eq!(unknown.status(), StatusCode::BAD_REQUEST);
    let body = unknown.text().await.unwrap();
    for secret in secrets {
        assert!(!body.contains(secret));
    }
    let value: Value = serde_json::from_str(&body).unwrap();
    let message = value["error"]["message"].as_str().unwrap();
    assert!(
        message.contains("inspect your connected channels")
            && message.contains("check the provider status")
    );
    assert!(!message.contains("remove"));
    h.provider.exchange_block.store(0, Ordering::SeqCst);
    h.provider.exchange_release.notify_one();
    assert_eq!(
        active_request.await.unwrap().unwrap().status(),
        StatusCode::SEE_OTHER
    );

    let revoke_before = h.provider.revoke_requests.load(Ordering::SeqCst);
    let response = disconnect().send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let result: Value = response.json().await.unwrap();
    assert_eq!(result["data"]["provider_revoked"], false);
    assert_eq!(result["data"]["channels_requiring_reconnection"], 1);
    let action = result["data"]["next_action"].as_str().unwrap();
    assert!(
        action.contains("unrelated accounts")
            && action.contains("First inspect")
            && action.contains("then confirm")
    );
    let sibling: (String, String) =
        sqlx::query_as("SELECT connection_status,encrypted_credentials FROM channels WHERE id=$1")
            .bind(ids[1])
            .fetch_one(&h.state.db)
            .await
            .unwrap();
    assert_eq!(sibling, ("reconnect_required".into(), sibling_cipher));
    let queued: String = sqlx::query_scalar("SELECT state FROM publications WHERE id=$1")
        .bind(publication)
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    assert_eq!(queued, "queued");
    let pending: Option<Uuid> = sqlx::query_scalar(
        "SELECT revocation_id FROM provider_oauth_fences WHERE provider='mastodon'",
    )
    .fetch_one(&h.state.db)
    .await
    .unwrap();
    assert!(pending.is_some());
    let repeat = disconnect().send().await.unwrap();
    assert_eq!(repeat.status(), StatusCode::CONFLICT);
    let value: Value = repeat.json().await.unwrap();
    let message = value["error"]["message"].as_str().unwrap();
    assert!(
        message.contains("Provider removal is pending")
            && message.contains("unrelated accounts")
            && message.contains("Only after removal")
    );
    assert_eq!(
        complete(&candidate, json!({"destination_id":"masto-destination"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    let exchanges = h.provider.exchange_requests.load(Ordering::SeqCst);
    assert_eq!(
        callback(&stale_before_state, &stale_before_cookie)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        h.provider.exchange_requests.load(Ordering::SeqCst),
        exchanges
    );
    assert_eq!(ack(h.revocation_marker("mastodon").await).send().await.unwrap().status(), StatusCode::OK);
    assert_eq!(
        complete(&candidate, json!({"destination_id":"masto-destination"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        complete(&active, json!({"destination_id":"masto-destination"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        callback(&stale_after_state, &stale_after_cookie)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        h.provider.exchange_requests.load(Ordering::SeqCst),
        exchanges
    );
    let repeated: Value = disconnect().send().await.unwrap().json().await.unwrap();
    assert!(
        repeated["data"]["next_action"]
            .as_str()
            .unwrap()
            .contains("already disconnected")
    );
    let (fresh, fresh_state, fresh_cookie) = start(&client, &base, &session, brand).await;
    assert_eq!(
        callback(&fresh_state, &fresh_cookie)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::SEE_OTHER
    );
    assert_eq!(
        complete(&fresh, json!({"destination_id":"masto-destination"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        h.provider.revoke_requests.load(Ordering::SeqCst),
        revoke_before
    );
    assert_eq!(h.provider.masto_revoke_requests.load(Ordering::SeqCst), 0);

    sqlx::query("DELETE FROM connection_flows WHERE brand_id=$1")
        .bind(brand)
        .execute(&h.state.db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM publications WHERE id=$1")
        .bind(publication)
        .execute(&h.state.db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM posts WHERE id=$1")
        .bind(post_id)
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
    server.abort();
    println!(
        "PASS provider gate blocks pre-validation recovery and manual removal fences stale grants"
    );
}
