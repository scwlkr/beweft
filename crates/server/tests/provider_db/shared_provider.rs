async fn disconnect_fences_shared_provider_channels(h: &Harness) {
    use axum::routing::post;
    use std::time::Duration;

    h.app("linkedin", "shared-client", None).await;
    let brands = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
    for brand in brands {
        sqlx::query("INSERT INTO brands(id,name) VALUES($1,'Shared grant fixture')")
            .bind(brand)
            .execute(&h.state.db)
            .await
            .unwrap();
    }
    let ids = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
    let app = connections::app_config(&h.state, "linkedin").await.unwrap();
    let mut ciphertexts = Vec::new();
    for index in 0..3 {
        let grant = Channel {
            app: app.clone(),
            identity: Identity {
                id: if index == 2 {
                    "unrelated-account"
                } else {
                    "shared-account"
                }
                .into(),
                name: "Fixture".into(),
                username: None,
            },
            tokens: Tokens {
                access_token: "shared-access-fixture".into(),
                refresh_token: Some("shared-refresh-fixture".into()),
                expires_at: None,
                scopes: vec![],
                scopes_known: false,
            },
        };
        let encrypted = crypto::encrypt(
            &[13; 32],
            ids[index],
            &serde_json::to_string(&grant).unwrap(),
        )
        .unwrap();
        sqlx::query("INSERT INTO channels(id,brand_id,name,provider,encrypted_credentials,connection_identity,connection_status,paused) VALUES($1,$2,'Fixture','linkedin',$3,$4,'connected',$5)")
            .bind(ids[index]).bind(brands[index]).bind(&encrypted).bind(grant.destination_key()).bind(index == 2).execute(&h.state.db).await.unwrap();
        ciphertexts.push(encrypted);
    }
    let post_id = Uuid::new_v4();
    let publication = Uuid::new_v4();
    sqlx::query("INSERT INTO posts(id,brand_id,text) VALUES($1,$2,'Shared fixture')")
        .bind(post_id)
        .bind(brands[1])
        .execute(&h.state.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO publications(id,post_id,brand_id,channel_id,post_version,scheduled_at,state,payload) VALUES($1,$2,$3,$4,1,now(),'sending','{}')")
        .bind(publication).bind(post_id).bind(brands[1]).bind(ids[1]).execute(&h.state.db).await.unwrap();
    let router = Router::new()
        .route("/disconnect/{id}", post(connections::disconnect))
        .route("/start/{provider}", post(connections::start))
        .route("/ack/{provider}", post(connections::acknowledge_revocation))
        .with_state(h.state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .unwrap();
    let session = format!("beweft_session={}", h.session);
    let disconnect = |id: Uuid| {
        client
            .post(format!("{base}/disconnect/{id}"))
            .header(header::COOKIE, &session)
            .header("x-beweft-request", "1")
    };
    let ack = |provider: &str| {
        client
            .post(format!("{base}/ack/{provider}"))
            .header(header::COOKIE, &session)
            .header("x-beweft-request", "1")
            .json(&json!({"access_removed":true}))
    };

    // An active sibling delivery blocks a provider-wide removal boundary.
    assert_eq!(
        disconnect(ids[0]).send().await.unwrap().status(),
        StatusCode::CONFLICT
    );
    let still_connected: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM channels WHERE id=ANY($1) AND connection_status='connected'",
    )
    .bind(&ids[..])
    .fetch_one(&h.state.db)
    .await
    .unwrap();
    assert_eq!(still_connected, 3);
    sqlx::query("UPDATE publications SET state='queued' WHERE id=$1")
        .bind(publication)
        .execute(&h.state.db)
        .await
        .unwrap();
    let response = disconnect(ids[0]).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().await.unwrap();
    for secret in [
        "shared-access-fixture",
        "shared-refresh-fixture",
        &h.session,
    ] {
        assert!(
            !body.contains(secret),
            "disconnect response exposed a secret"
        );
    }
    let result: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(result["data"]["provider_revoked"], false);
    assert_eq!(result["data"]["channels_requiring_reconnection"], 2);
    let action = result["data"]["next_action"].as_str().unwrap();
    assert!(
        action.contains("unrelated accounts")
            && action.contains("may affect")
            && action.contains("confirm")
    );
    for index in 1..3 {
        let saved: (String, String, bool) = sqlx::query_as(
            "SELECT connection_status,encrypted_credentials,paused FROM channels WHERE id=$1",
        )
        .bind(ids[index])
        .fetch_one(&h.state.db)
        .await
        .unwrap();
        assert_eq!(
            saved,
            (
                "reconnect_required".into(),
                ciphertexts[index].clone(),
                index == 2
            )
        );
        assert!(
            connections::load_channel(&h.state, ids[index])
                .await
                .is_err()
        );
    }
    let queued: String = sqlx::query_scalar("SELECT state FROM publications WHERE id=$1")
        .bind(publication)
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    assert_eq!(queued, "queued");
    let start = client
        .post(format!("{base}/start/linkedin"))
        .header(header::COOKIE, &session)
        .header("x-beweft-request", "1")
        .json(&json!({"brand_id":brands[1],"reconnect_channel_id":ids[1]}))
        .send()
        .await
        .unwrap();
    assert_eq!(start.status(), StatusCode::CONFLICT);
    let blocked: Value = start.json().await.unwrap();
    let message = blocked["error"]["message"].as_str().unwrap();
    assert!(message.contains("other channels") && message.contains("unrelated accounts"));
    assert_eq!(
        ack("linkedin").send().await.unwrap().status(),
        StatusCode::OK
    );
    let status: String = sqlx::query_scalar("SELECT connection_status FROM channels WHERE id=$1")
        .bind(ids[1])
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    assert_eq!(status, "reconnect_required");

    // With no target grant and no remote operation, a connected sibling is not
    // marked unhealthy and the response directs the owner only to orphaned access.
    let no_grant = Uuid::new_v4();
    let connected = Uuid::new_v4();
    sqlx::query("INSERT INTO channels(id,brand_id,name,provider,connection_status) VALUES($1,$2,'No grant','linkedin','disconnected')")
        .bind(no_grant).bind(brands[0]).execute(&h.state.db).await.unwrap();
    sqlx::query("INSERT INTO channels(id,brand_id,name,provider,encrypted_credentials,connection_identity,connection_status) VALUES($1,$2,'Connected','linkedin',$3,'another-account','connected')")
        .bind(connected).bind(brands[0]).bind("fixture ciphertext").execute(&h.state.db).await.unwrap();
    let result: Value = disconnect(no_grant)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(result["data"]["channels_requiring_reconnection"], 0);
    let action = result["data"]["next_action"].as_str().unwrap();
    assert!(action.contains("No remote removal") && action.contains("only orphaned"));
    let status: String = sqlx::query_scalar("SELECT connection_status FROM channels WHERE id=$1")
        .bind(connected)
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    assert_eq!(status, "connected");
    let pending: Option<Uuid> = sqlx::query_scalar(
        "SELECT revocation_id FROM provider_oauth_fences WHERE provider='linkedin'",
    )
    .fetch_one(&h.state.db)
    .await
    .unwrap();
    assert!(pending.is_none());

    // A successful fixture revoke is still treated conservatively; no claim
    // is made about the remote operation's scope.
    h.app("mastodon", "masto-shared", Some("https://social.example"))
        .await;
    let app = connections::app_config(&h.state, "mastodon").await.unwrap();
    let masto = [Uuid::new_v4(), Uuid::new_v4()];
    for index in 0..2 {
        let grant = Channel {
            app: app.clone(),
            identity: Identity {
                id: format!("masto-{index}"),
                name: "Fixture".into(),
                username: None,
            },
            tokens: Tokens {
                access_token: "masto-access-fixture".into(),
                refresh_token: None,
                expires_at: None,
                scopes: vec![],
                scopes_known: false,
            },
        };
        let encrypted = crypto::encrypt(
            &[13; 32],
            masto[index],
            &serde_json::to_string(&grant).unwrap(),
        )
        .unwrap();
        sqlx::query("INSERT INTO channels(id,brand_id,name,provider,encrypted_credentials,connection_identity,connection_status) VALUES($1,$2,'Masto','mastodon',$3,$4,'connected')")
            .bind(masto[index]).bind(brands[index]).bind(encrypted).bind(grant.destination_key()).execute(&h.state.db).await.unwrap();
    }
    let result: Value = disconnect(masto[0])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(result["data"]["provider_revoked"], true);
    assert_eq!(result["data"]["channels_requiring_reconnection"], 1);
    let status: String = sqlx::query_scalar("SELECT connection_status FROM channels WHERE id=$1")
        .bind(masto[1])
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    assert_eq!(status, "reconnect_required");

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
    sqlx::query("DELETE FROM channels WHERE brand_id=ANY($1)")
        .bind(&brands[..])
        .execute(&h.state.db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM brands WHERE id=ANY($1)")
        .bind(&brands[..])
        .execute(&h.state.db)
        .await
        .unwrap();
    server.abort();
    println!(
        "PASS shared-provider manual and successful removal preserve sibling queues and honest health"
    );
}
