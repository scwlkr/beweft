async fn callback(
    h: &Harness,
    state: String,
    headers: HeaderMap,
) -> beweft::error::Result<Response> {
    connections::callback(
        State(h.state.clone()),
        headers,
        Path("x".into()),
        Query(connections::Callback {
            state: Some(state),
            code: Some("fixture-code".into()),
            error: None,
        }),
    )
    .await
}
async fn oauth_replay_session_binding_and_config_change(h: &Harness) {
    h.app("x", "client", None).await;
    let (flow, state, headers) = h.start().await;
    assert!(callback(h, state.clone(), HeaderMap::new()).await.is_err());
    callback(h, state.clone(), headers.clone()).await.unwrap();
    assert!(
        callback(h, state, headers).await.is_err(),
        "OAuth callback cannot replay"
    );
    let mut wrong = HeaderMap::new();
    wrong.insert(
        header::COOKIE,
        "beweft_session=another-session".parse().unwrap(),
    );
    assert!(
        connections::flow_destinations(State(h.state.clone()), h.auth.clone(), wrong, Path(flow))
            .await
            .is_err()
    );
    let _ = connections::complete(
        State(h.state.clone()),
        h.auth.clone(),
        h.headers(),
        Path(flow),
        Json(json!({"destination_id":"destination"})),
    )
    .await
    .unwrap();
    assert!(
        connections::complete(
            State(h.state.clone()),
            h.auth.clone(),
            h.headers(),
            Path(flow),
            Json(json!({"destination_id":"destination"}))
        )
        .await
        .is_err()
    );
    let (_, state, headers) = h.start().await;
    let before = h.provider.refreshes.load(Ordering::SeqCst);
    h.app("x", "new-client", None).await;
    assert!(callback(h, state, headers).await.is_err());
    assert_eq!(
        before,
        h.provider.refreshes.load(Ordering::SeqCst),
        "changed config must not receive old auth code"
    );
    let (_, state, headers) = h.start().await;
    sqlx::query("UPDATE sessions SET expires_at=now()-interval '1 second' WHERE token_hash=$1")
        .bind(crypto::digest(&h.session))
        .execute(&h.state.db)
        .await
        .unwrap();
    assert!(callback(h, state, headers).await.is_err());
    sqlx::query("UPDATE sessions SET expires_at=now()+interval '1 hour' WHERE token_hash=$1")
        .bind(crypto::digest(&h.session))
        .execute(&h.state.db)
        .await
        .unwrap();
    println!("PASS OAuth replay, originating session and config-change rejection");
}
async fn refresh_serialization_and_grant_binding(h: &Harness) {
    h.app("x", "client", None).await;
    // Existing X destination from prior scenario; fresh brand avoids the intentional destination uniqueness constraint.
    let id: Uuid =
        sqlx::query_scalar("SELECT id FROM channels WHERE brand_id=$1 AND provider='x' LIMIT 1")
            .bind(h.brand)
            .fetch_one(&h.state.db)
            .await
            .unwrap();
    let app = connections::app_config(&h.state, "x").await.unwrap();
    let c = Channel {
        app,
        identity: Identity {
            id: "destination".into(),
            name: "Fixture".into(),
            username: None,
        },
        tokens: Tokens {
            access_token: "expired".into(),
            refresh_token: Some("refresh".into()),
            expires_at: Some(0),
            scopes: vec![],
            scopes_known: false,
        },
    };
    h.save_channel(id, &c).await;
    let before = h.provider.refreshes.load(Ordering::SeqCst);
    let (a, b, c) = tokio::join!(
        connections::load_channel(&h.state, id),
        connections::load_channel(&h.state, id),
        connections::load_channel(&h.state, id)
    );
    assert!(a.is_ok() && b.is_ok() && c.is_ok());
    assert_eq!(
        h.provider.refreshes.load(Ordering::SeqCst),
        before + 1,
        "rotated grant must refresh exactly once"
    );
    h.app("x", "different", None).await;
    assert!(connections::load_channel(&h.state, id).await.is_err());
    h.app("mastodon", "masto-client", Some("https://first.example"))
        .await;
    let id = h.channel("mastodon").await;
    h.app("mastodon", "masto-client", Some("https://second.example"))
        .await;
    assert!(connections::load_channel(&h.state, id).await.is_err());
    let state: String = sqlx::query_scalar("SELECT connection_status FROM channels WHERE id=$1")
        .bind(id)
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    assert_eq!(state, "reconnect_required");
    let app = connections::app_config(&h.state, "mastodon").await.unwrap();
    let candidate = Channel {
        app,
        identity: Identity {
            id: "destination".into(),
            name: "Different origin".into(),
            username: None,
        },
        tokens: Tokens {
            access_token: "new-grant".into(),
            refresh_token: None,
            expires_at: None,
            scopes: vec![],
            scopes_known: false,
        },
    };
    let flow = Uuid::new_v4();
    let encrypted = crypto::encrypt(
        &[13; 32],
        flow,
        &serde_json::to_string(&vec![candidate]).unwrap(),
    )
    .unwrap();
    sqlx::query("INSERT INTO connection_flows(id,provider,owner_id,brand_id,session_hash,state_hash,correlation_hash,encrypted_verifier,reconnect_channel_id,expires_at,encrypted_candidates,oauth_generation) VALUES($1,'mastodon',$2,$3,$4,$5,'unused','unused',$6,now()+interval '10 minutes',$7,(SELECT oauth_generation FROM provider_apps WHERE provider='mastodon'))").bind(flow).bind(h.auth.owner_id).bind(h.brand).bind(crypto::digest(&h.session)).bind(flow.to_string()).bind(id).bind(encrypted).execute(&h.state.db).await.unwrap();
    assert!(
        connections::complete(
            State(h.state.clone()),
            h.auth.clone(),
            h.headers(),
            Path(flow),
            Json(json!({"destination_id":"destination"}))
        )
        .await
        .is_err(),
        "same remote account ID at another origin is not the same destination"
    );
    println!("PASS serialized refresh and immutable grant origin/client binding");
}
