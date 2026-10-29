async fn provider_latch_spans_remote_revoke_and_commit_gap(h: &Harness) {
    use axum::routing::{get, post, put};
    use std::time::Duration;

    h.app("mastodon", "latch-client", Some("https://social.example"))
        .await;
    let brand = Uuid::new_v4();
    let ids = [Uuid::new_v4(), Uuid::new_v4()];
    sqlx::query("INSERT INTO brands(id,name) VALUES($1,'Latch fixture')")
        .bind(brand)
        .execute(&h.state.db)
        .await
        .unwrap();
    let app = connections::app_config(&h.state, "mastodon").await.unwrap();
    for (index, id) in ids.iter().enumerate() {
        let grant = Channel {
            app: app.clone(),
            identity: Identity {
                id: format!("latch-{index}"),
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
        sqlx::query("INSERT INTO channels(id,brand_id,name,provider,encrypted_credentials,connection_identity,connection_status) VALUES($1,$2,'Fixture','mastodon',$3,$4,'connected')")
            .bind(id).bind(brand).bind(encrypted).bind(grant.destination_key()).execute(&h.state.db).await.unwrap();
    }
    let post_id = Uuid::new_v4();
    let publication = Uuid::new_v4();
    sqlx::query("INSERT INTO posts(id,brand_id,text) VALUES($1,$2,'Latch fixture')")
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
        .route("/ack/{provider}", post(connections::acknowledge_revocation))
        .route("/app/{provider}", put(connections::save_app))
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
    let disconnect = || {
        client
            .post(format!("{base}/disconnect/{}", ids[0]))
            .header(header::COOKIE, &session)
            .header("x-beweft-request", "1")
    };
    let start = || {
        client
            .post(format!("{base}/start/mastodon"))
            .header(header::COOKIE, &session)
            .header("x-beweft-request", "1")
            .json(&json!({"brand_id":brand,"reconnect_channel_id":ids[1]}))
    };
    let ack = || {
        client
            .post(format!("{base}/ack/mastodon"))
            .header(header::COOKIE, &session)
            .header("x-beweft-request", "1")
            .json(&json!({"access_removed":true}))
    };
    async fn assert_busy(response: reqwest::Response, secrets: &[&str]) {
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = response.text().await.unwrap();
        for secret in secrets {
            assert!(!body.contains(secret), "busy response exposed a secret");
        }
        let value: Value = serde_json::from_str(&body).unwrap();
        let message = value["error"]["message"].as_str().unwrap();
        assert!(message.contains("Wait for it to finish"));
        assert!(message.contains("retry and check the current provider status"));
        assert!(!message.contains("connected-app settings"));
        assert!(!message.contains("remove the affected access"));
    }
    // Prepare a callback before disconnect; it must consume state but never
    // exchange while the revoke owns the provider operation latch.
    let response = start().send().await.unwrap();
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
    let flow = value["data"]["flow_id"].as_str().unwrap().to_owned();
    let url = url::Url::parse(value["data"]["authorization_url"].as_str().unwrap()).unwrap();
    let state = url
        .query_pairs()
        .find(|(key, _)| key == "state")
        .unwrap()
        .1
        .to_string();
    let callback = || {
        client
            .get(format!("{base}/callback/mastodon"))
            .header(header::COOKIE, &cookie)
            .query(&[("state", state.as_str()), ("code", "fixture-code")])
    };
    let destinations = || {
        client
            .get(format!("{base}/destinations/{flow}"))
            .header(header::COOKIE, &session)
    };
    let secrets = [
        "latch-access-fixture",
        "fixture-code",
        &h.session,
        &state,
        cookie.split_once('=').unwrap().1,
    ];
    let before = h.provider.masto_revoke_requests.load(Ordering::SeqCst);
    let exchanges = h.provider.exchange_requests.load(Ordering::SeqCst);
    h.provider.revoke_block.store(1, Ordering::SeqCst);
    let active = tokio::spawn(disconnect().send());
    tokio::time::timeout(Duration::from_secs(3), h.provider.revoke_started.notified())
        .await
        .unwrap();
    let marker: Option<Uuid> = sqlx::query_scalar(
        "SELECT revocation_id FROM provider_oauth_fences WHERE provider='mastodon'",
    )
    .fetch_one(&h.state.db)
    .await
    .unwrap();
    assert!(marker.is_some());
    assert_busy(disconnect().send().await.unwrap(), &secrets).await;
    assert_busy(start().send().await.unwrap(), &secrets).await;
    assert_busy(ack().send().await.unwrap(), &secrets).await;
    let app_edit = client
        .put(format!("{base}/app/mastodon"))
        .header(header::COOKIE, &session)
        .header("x-beweft-request", "1")
        .json(&json!({"client_id":"latch-client","instance_url":"https://social.example"}))
        .send()
        .await
        .unwrap();
    assert_busy(app_edit, &secrets).await;
    let response = callback().send().await.unwrap();
    assert_busy(response, &secrets).await;
    assert_busy(callback().send().await.unwrap(), &secrets).await;
    assert_busy(destinations().send().await.unwrap(), &secrets).await;
    assert_eq!(
        h.provider.exchange_requests.load(Ordering::SeqCst),
        exchanges
    );
    let probe: i32 = tokio::time::timeout(
        Duration::from_secs(2),
        sqlx::query_scalar("SELECT 1").fetch_one(&h.state.db),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(probe, 1);
    h.provider.revoke_block.store(0, Ordering::SeqCst);
    h.provider.revoke_release.notify_one();
    let result: Value = active.await.unwrap().unwrap().json().await.unwrap();
    assert_eq!(result["data"]["provider_revoked"], true);
    assert_eq!(
        h.provider.masto_revoke_requests.load(Ordering::SeqCst),
        before + 1
    );
    let after:(i64,Option<Uuid>)=sqlx::query_as("SELECT oauth_generation,revocation_id FROM provider_oauth_fences WHERE provider='mastodon'")
        .fetch_one(&h.state.db).await.unwrap();
    assert!(after.1.is_none());
    let result: Value = disconnect().send().await.unwrap().json().await.unwrap();
    assert!(
        result["data"]["next_action"]
            .as_str()
            .unwrap()
            .contains("already disconnected")
    );
    assert!(
        !result["data"]["next_action"]
            .as_str()
            .unwrap()
            .contains("connected-app")
    );
    let current:(i64,Option<Uuid>)=sqlx::query_as("SELECT oauth_generation,revocation_id FROM provider_oauth_fences WHERE provider='mastodon'")
        .fetch_one(&h.state.db).await.unwrap();
    assert_eq!(current, after);
    let queued: String = sqlx::query_scalar("SELECT state FROM publications WHERE id=$1")
        .bind(publication)
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    assert_eq!(queued, "queued");

    // Hold the same session latch through an explicit commit boundary, before
    // any remote phase starts. All regular gate users must reject in that gap.
    let mut gap = h.state.db.acquire().await.unwrap();
    gap.close_on_drop();
    let admitted: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock(hashtextextended($1,0))")
        .bind("provider-operation:mastodon")
        .fetch_one(&mut *gap)
        .await
        .unwrap();
    assert!(admitted);
    let mut tx = sqlx::Acquire::begin(&mut *gap).await.unwrap();
    sqlx::query("UPDATE provider_oauth_fences SET revocation_id=$1,oauth_generation=oauth_generation+1 WHERE provider='mastodon'")
        .bind(Uuid::new_v4()).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert_busy(disconnect().send().await.unwrap(), &secrets).await;
    assert_busy(start().send().await.unwrap(), &secrets).await;
    assert_busy(ack().send().await.unwrap(), &secrets).await;
    let mut tx = sqlx::Acquire::begin(&mut *gap).await.unwrap();
    sqlx::query("UPDATE provider_oauth_fences SET revocation_id=NULL WHERE provider='mastodon'")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    drop(gap);
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let mut tx = h.state.db.begin().await.unwrap();
            let available: bool =
                sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtextextended($1,0))")
                    .bind("provider-operation:mastodon")
                    .fetch_one(&mut *tx)
                    .await
                    .unwrap();
            tx.commit().await.unwrap();
            if available {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let result: Value = disconnect().send().await.unwrap().json().await.unwrap();
    assert!(
        result["data"]["next_action"]
            .as_str()
            .unwrap()
            .contains("already disconnected")
    );

    // Canceling the local revoke drops the session latch rather than returning
    // it to the pool; the committed marker remains unresolved. The fixture
    // release does not claim the remote operation was canceled.
    let canceled = Uuid::new_v4();
    let grant = Channel {
        app: app.clone(),
        identity: Identity {
            id: "latch-canceled".into(),
            name: "Fixture".into(),
            username: None,
        },
        tokens: Tokens {
            access_token: "latch-cancel-access".into(),
            refresh_token: None,
            expires_at: None,
            scopes: vec![],
            scopes_known: false,
        },
    };
    let encrypted =
        crypto::encrypt(&[13; 32], canceled, &serde_json::to_string(&grant).unwrap()).unwrap();
    sqlx::query("INSERT INTO channels(id,brand_id,name,provider,encrypted_credentials,connection_identity,connection_status) VALUES($1,$2,'Canceled','mastodon',$3,$4,'connected')")
        .bind(canceled).bind(brand).bind(encrypted).bind(grant.destination_key()).execute(&h.state.db).await.unwrap();
    h.provider.revoke_block.store(1, Ordering::SeqCst);
    let mut auth = h.auth.clone();
    auth.brand_ids = vec![brand];
    let state = h.state.clone();
    let canceled_task =
        tokio::spawn(
            async move { connections::disconnect(State(state), auth, Path(canceled)).await },
        );
    tokio::time::timeout(Duration::from_secs(3), h.provider.revoke_started.notified())
        .await
        .unwrap();
    canceled_task.abort();
    assert!(canceled_task.await.unwrap_err().is_cancelled());
    h.provider.revoke_block.store(0, Ordering::SeqCst);
    h.provider.revoke_release.notify_one();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let mut tx = h.state.db.begin().await.unwrap();
            let available: bool =
                sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtextextended($1,0))")
                    .bind("provider-operation:mastodon")
                    .fetch_one(&mut *tx)
                    .await
                    .unwrap();
            tx.commit().await.unwrap();
            if available {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let pending: Option<Uuid> = sqlx::query_scalar(
        "SELECT revocation_id FROM provider_oauth_fences WHERE provider='mastodon'",
    )
    .fetch_one(&h.state.db)
    .await
    .unwrap();
    assert!(pending.is_some());
    let response = client
        .post(format!("{base}/disconnect/{canceled}"))
        .header(header::COOKIE, &session)
        .header("x-beweft-request", "1")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let value: Value = response.json().await.unwrap();
    assert!(
        value["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Provider removal is pending")
    );
    assert_eq!(ack().send().await.unwrap().status(), StatusCode::OK);

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
        "PASS provider latch covers blocked revoke and commit gap; busy responses require a fresh status"
    );
}
