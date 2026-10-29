async fn post_grant_failures_preserve_status_and_recovery(h: &Harness) {
    use axum::routing::{get, post};
    use std::time::Duration;

    h.app("x", "post-grant-client", None).await;
    let brand = Uuid::new_v4();
    sqlx::query("INSERT INTO brands(id,name) VALUES($1,'Post-grant fixture')")
        .bind(brand)
        .execute(&h.state.db)
        .await
        .unwrap();
    let router = Router::new()
        .route("/start/{provider}", post(connections::start))
        .route("/callback/{provider}", get(connections::callback))
        .route("/destinations/{id}", get(connections::flow_destinations))
        .route("/complete/{id}", post(connections::complete))
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
    ) -> (String, String, String) {
        let response = client
            .post(format!("{base}/start/x"))
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
    async fn assert_recovery(
        response: reqwest::Response,
        status: StatusCode,
        code: &str,
        secrets: &[&str],
    ) {
        assert_eq!(response.status(), status);
        let body = response.text().await.unwrap();
        for secret in secrets {
            assert!(!body.contains(secret), "recovery exposed a secret");
        }
        let value: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(value["error"]["code"], code);
        let message = value["error"]["message"].as_str().unwrap();
        assert!(message.contains("First inspect your connected channels"));
        assert!(message.contains("remove only orphaned access"));
        assert_eq!(
            message
                .matches("First inspect your connected channels")
                .count(),
            1
        );
    }
    let callback = |state: &str, cookie: &str| {
        client
            .get(format!("{base}/callback/x"))
            .header(header::COOKIE, cookie)
            .query(&[("state", state), ("code", "fixture-code")])
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

    // The user completed consent and the app authority changed before choosing
    // a destination. A restart-only response would abandon possible access.
    let (flow, state, cookie) = start(&client, &base, &session, brand).await;
    assert_eq!(
        callback(&state, &cookie).send().await.unwrap().status(),
        StatusCode::SEE_OTHER
    );
    h.app("x", "changed-client", None).await;
    let secrets = [
        "fixture-code",
        "rotated",
        "rotated-refresh",
        &h.session,
        &state,
        cookie.split_once('=').unwrap().1,
    ];
    assert_recovery(
        complete(&flow).send().await.unwrap(),
        StatusCode::BAD_REQUEST,
        "invalid_request",
        &secrets,
    )
    .await;
    h.app("x", "post-grant-client", None).await;

    // A real uniqueness conflict during channel storage keeps its 409 code and
    // the existing channel, while explaining the already-created provider grant.
    let (first, first_state, first_cookie) = start(&client, &base, &session, brand).await;
    assert_eq!(
        callback(&first_state, &first_cookie)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::SEE_OTHER
    );
    let response = complete(&first).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let value: Value = response.json().await.unwrap();
    let channel: Uuid = value["data"]["id"].as_str().unwrap().parse().unwrap();
    let (duplicate, duplicate_state, duplicate_cookie) =
        start(&client, &base, &session, brand).await;
    assert_eq!(
        callback(&duplicate_state, &duplicate_cookie)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::SEE_OTHER
    );
    let secrets = [
        "fixture-code",
        "rotated",
        "rotated-refresh",
        &h.session,
        &duplicate_state,
        duplicate_cookie.split_once('=').unwrap().1,
    ];
    assert_recovery(
        complete(&duplicate).send().await.unwrap(),
        StatusCode::CONFLICT,
        "conflict",
        &secrets,
    )
    .await;
    let connected: bool =
        sqlx::query_scalar("SELECT connection_status='connected' FROM channels WHERE id=$1")
            .bind(channel)
            .fetch_one(&h.state.db)
            .await
            .unwrap();
    assert!(connected);

    // A corrupted stored candidate is likewise a post-grant failure.
    let (corrupt, corrupt_state, corrupt_cookie) = start(&client, &base, &session, brand).await;
    assert_eq!(
        callback(&corrupt_state, &corrupt_cookie)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::SEE_OTHER
    );
    sqlx::query(
        "UPDATE connection_flows SET encrypted_candidates='invalid fixture ciphertext' WHERE id=$1",
    )
    .bind(Uuid::parse_str(&corrupt).unwrap())
    .execute(&h.state.db)
    .await
    .unwrap();
    let secrets = [
        "fixture-code",
        "rotated",
        "rotated-refresh",
        &h.session,
        &corrupt_state,
        corrupt_cookie.split_once('=').unwrap().1,
    ];
    assert_recovery(
        destinations(&corrupt).send().await.unwrap(),
        StatusCode::BAD_REQUEST,
        "invalid_request",
        &secrets,
    )
    .await;

    // An authority change before exchange also receives recovery advice, but
    // the stale code must never reach the provider.
    let (_, stale_state, stale_cookie) = start(&client, &base, &session, brand).await;
    h.app("x", "changed-client", None).await;
    let before = h.provider.exchange_requests.load(Ordering::SeqCst);
    let secrets = [
        "fixture-code",
        "rotated",
        "rotated-refresh",
        &h.session,
        &stale_state,
        stale_cookie.split_once('=').unwrap().1,
    ];
    assert_recovery(
        callback(&stale_state, &stale_cookie).send().await.unwrap(),
        StatusCode::BAD_REQUEST,
        "invalid_request",
        &secrets,
    )
    .await;
    assert_eq!(h.provider.exchange_requests.load(Ordering::SeqCst), before);
    h.app("x", "post-grant-client", None).await;

    // Force a PostgreSQL failure after successful token exchange; status stays
    // 500 and the safe response still explains how to inspect possible access.
    let (failed, failed_state, failed_cookie) = start(&client, &base, &session, brand).await;
    sqlx::raw_sql("CREATE FUNCTION fixture_reject_candidates() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.encrypted_candidates IS NOT NULL THEN RAISE EXCEPTION 'fixture candidate failure'; END IF; RETURN NEW; END $$; CREATE TRIGGER fixture_reject_candidates BEFORE UPDATE OF encrypted_candidates ON connection_flows FOR EACH ROW EXECUTE FUNCTION fixture_reject_candidates();")
        .execute(&h.state.db).await.unwrap();
    let response = callback(&failed_state, &failed_cookie)
        .send()
        .await
        .unwrap();
    assert!(response.headers().get(header::LOCATION).is_none());
    let secrets = [
        "fixture-code",
        "rotated",
        "rotated-refresh",
        &h.session,
        &failed_state,
        failed_cookie.split_once('=').unwrap().1,
    ];
    assert_recovery(
        response,
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        &secrets,
    )
    .await;
    sqlx::raw_sql("DROP TRIGGER fixture_reject_candidates ON connection_flows; DROP FUNCTION fixture_reject_candidates();")
        .execute(&h.state.db).await.unwrap();
    let stored: bool = sqlx::query_scalar(
        "SELECT encrypted_candidates IS NOT NULL FROM connection_flows WHERE id=$1",
    )
    .bind(Uuid::parse_str(&failed).unwrap())
    .fetch_one(&h.state.db)
    .await
    .unwrap();
    assert!(!stored);

    sqlx::query("DELETE FROM connection_flows WHERE brand_id=$1")
        .bind(brand)
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
        "PASS post-grant config, candidate and storage failures retain status and actionable recovery"
    );
}
