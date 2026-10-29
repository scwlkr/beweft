#[derive(Default)]
struct Provider {
    refreshes: AtomicUsize,
    strict_refresh: std::sync::atomic::AtomicBool,
    refresh_generation: AtomicUsize,
    callback_origin: std::sync::Mutex<String>,
    media_received: std::sync::Mutex<Vec<u8>>,
    post_mode: AtomicUsize,
    post_writes: AtomicUsize,
    post_started: Notify,
    post_release: Notify,
    writes: AtomicUsize,
    reply_started: Notify,
    reply_release: Notify,
    probe_started: Notify,
    probe_release: Notify,
    revoke_requests: AtomicUsize,
    sync_mode: AtomicUsize,
    sync_requests: AtomicUsize,
    sync_started: Notify,
    sync_release: Notify,
    exchange_block: AtomicUsize,
    exchange_requests: AtomicUsize,
    exchange_started: Notify,
    exchange_release: Notify,
    identity_block: AtomicUsize,
    identity_started: Notify,
    identity_release: Notify,
}
async fn mock(State(s): State<Arc<Provider>>, r: Request) -> Response {
    let path = r.uri().path().to_owned();
    match path.as_str() {
        "/2/oauth2/token" | "/token" => {
            let body = to_bytes(r.into_body(), 10000).await.unwrap();
            let form: std::collections::HashMap<String, String> =
                url::form_urlencoded::parse(&body).into_owned().collect();
            if form.get("grant_type").map(String::as_str) == Some("authorization_code") {
                s.exchange_requests.fetch_add(1, Ordering::SeqCst);
                if s.exchange_block.load(Ordering::SeqCst) != 0 {
                    s.exchange_started.notify_one();
                    s.exchange_release.notified().await;
                }
            }
            if s.strict_refresh.load(Ordering::SeqCst)
                && form.get("grant_type").map(String::as_str) == Some("refresh_token")
            {
                let generation = s.refresh_generation.load(Ordering::SeqCst);
                if form.get("refresh_token") != Some(&format!("rotation-{generation}")) {
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                s.refresh_generation.fetch_add(1, Ordering::SeqCst);
                return Json(json!({"access_token":format!("access-{}",generation+1),"refresh_token":format!("rotation-{}",generation+1),"expires_in":7200})).into_response();
            }
            s.refreshes.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            Json(json!({"access_token":"rotated","refresh_token":"rotated-refresh","expires_in":7200,"scope":"tweet.read tweet.write users.read offline.access media.write"})).into_response()
        }
        "/v23.0/destination/photos" => {
            let body: Value =
                serde_json::from_slice(&to_bytes(r.into_body(), 100000).await.unwrap()).unwrap();
            let origin = s.callback_origin.lock().unwrap().clone();
            let url = body["url"]
                .as_str()
                .unwrap()
                .replace("https://media.fixture", &origin);
            let response = reqwest::Client::new()
                .get(url)
                .timeout(std::time::Duration::from_secs(3))
                .send()
                .await
                .unwrap();
            if !response.status().is_success() {
                return StatusCode::BAD_GATEWAY.into_response();
            }
            *s.media_received.lock().unwrap() = response.bytes().await.unwrap().to_vec();
            Json(json!({"id":"photo","post_id":"photo-post"})).into_response()
        }
        "/2/tweets" => {
            s.post_writes.fetch_add(1, Ordering::SeqCst);
            match s.post_mode.load(Ordering::SeqCst) {
                1 => {
                    s.post_started.notify_one();
                    s.post_release.notified().await;
                    StatusCode::UNAUTHORIZED.into_response()
                }
                2 => {
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    Json(json!({"data":{"id":"late-post"}})).into_response()
                }
                _ => Json(json!({"data":{"id":"fixture-post"}})).into_response(),
            }
        }
        "/fastsession" => {
            let mut r = StatusCode::PERMANENT_REDIRECT.into_response();
            r.headers_mut()
                .insert(header::RANGE, "bytes=0-4".parse().unwrap());
            r
        }
        "/2/users/me" => {
            Json(json!({"data":{"id":"destination","name":"Fixture"}})).into_response()
        }
        "/api/v1/statuses" => {
            s.writes.fetch_add(1, Ordering::SeqCst);
            s.reply_started.notify_one();
            s.reply_release.notified().await;
            Json(json!({"id":"reply"})).into_response()
        }
        "/expired" => StatusCode::NOT_FOUND.into_response(),
        "/session" => {
            s.probe_started.notify_one();
            s.probe_release.notified().await;
            let mut r = StatusCode::PERMANENT_REDIRECT.into_response();
            r.headers_mut()
                .insert(header::RANGE, "bytes=0-4".parse().unwrap());
            r
        }
        "/revoke" => {
            s.revoke_requests.fetch_add(1, Ordering::SeqCst);
            StatusCode::OK.into_response()
        }
        "/youtube/v3/channels" => {
            if s.identity_block.load(Ordering::SeqCst) != 0 {
                s.identity_started.notify_one();
                s.identity_release.notified().await;
            }
            Json(json!({"items":[{"id":"destination","snippet":{"title":"Fixture"}}]})).into_response()
        }
        "/youtube/v3/commentThreads" => {
            s.sync_requests.fetch_add(1, Ordering::SeqCst);
            if s.sync_mode.load(Ordering::SeqCst) != 0 {
                s.sync_started.notify_one();
                s.sync_release.notified().await;
            }
            Json(json!({"items":[]})).into_response()
        }
        "/oauth/revoke" => Json(json!({})).into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}
struct Harness {
    state: AppState,
    auth: Auth,
    brand: Uuid,
    session: String,
    provider: Arc<Provider>,
    server: tokio::task::JoinHandle<()>,
}
impl Drop for Harness {
    fn drop(&mut self) {
        self.server.abort();
        let _ = std::fs::remove_dir_all(&self.state.config.media_dir);
    }
}
impl Harness {
    async fn new() -> Self {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .acquire_timeout(std::time::Duration::from_secs(5))
            .connect(&std::env::var("BEWEFT_TEST_DATABASE_URL").expect("isolated test DB"))
            .await
            .unwrap();
        sqlx::migrate!("../../migrations").run(&pool).await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        // Single test, current-thread runtime; fixture env read only during this process.
        unsafe {
            std::env::set_var(
                "BEWEFT_PROVIDER_FIXTURE_ORIGIN",
                format!("http://{}", listener.local_addr().unwrap()),
            );
        }
        let provider = Arc::new(Provider::default());
        let app = Router::new()
            .fallback(any(mock))
            .with_state(provider.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let owner = Uuid::new_v4();
        let brand = Uuid::new_v4();
        let session = crypto::random_secret();
        sqlx::query("INSERT INTO owners(id,email,password_hash) VALUES($1,$2,'unused')")
            .bind(owner)
            .bind(format!("{owner}@fixture.invalid"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO brands(id,name) VALUES($1,'Provider fixture')")
            .bind(brand)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO sessions(token_hash,owner_id,expires_at) VALUES($1,$2,now()+interval '1 hour')").bind(crypto::digest(&session)).bind(owner).execute(&pool).await.unwrap();
        let dir = std::env::temp_dir().join(format!("beweft-provider-db-{owner}"));
        std::fs::create_dir(&dir).unwrap();
        Self {
            state: AppState::new(
                pool,
                Arc::new(Config {
                    public_url: "http://localhost:3000".into(),
                    secure_cookie: false,
                    setup_secret: String::new(),
                    media_dir: dir,
                    fixture_enabled: true,
                    max_upload_bytes: 256 * 1024 * 1024,
                    media_quota_bytes: 1024 * 1024 * 1024,
                    fixture_delay_ms: 0,
                    encryption_key: Some([13; 32]),
                }),
            ),
            auth: Auth {
                owner_id: owner,
                email: "fixture".into(),
                session: true,
                scopes: vec![],
                brand_ids: vec![brand],
            },
            brand,
            session,
            provider,
            server,
        }
    }
    fn headers(&self) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(
            header::COOKIE,
            format!("beweft_session={}", self.session).parse().unwrap(),
        );
        h
    }
    async fn app(&self, provider: &str, client: &str, origin: Option<&str>) {
        let _ = connections::save_app(
            State(self.state.clone()),
            self.auth.clone(),
            Path(provider.into()),
            Json(
                json!({"client_id":client,"client_secret":"fixture-secret","instance_url":origin}),
            ),
        )
        .await
        .unwrap();
    }
    async fn channel(&self, provider: &str) -> Uuid {
        let app = connections::app_config(&self.state, provider)
            .await
            .unwrap();
        let id = Uuid::new_v4();
        let c = Channel {
            identity: Identity {
                id: "destination".into(),
                name: "Fixture".into(),
                username: None,
            },
            tokens: Tokens {
                access_token: "fixture-token".into(),
                refresh_token: Some("fixture-refresh".into()),
                expires_at: None,
                scopes: vec![],
                scopes_known: false,
            },
            app,
        };
        self.save_channel(id, &c).await;
        id
    }
    async fn save_channel(&self, id: Uuid, c: &Channel) {
        let encrypted = crypto::encrypt(&[13; 32], id, &serde_json::to_string(c).unwrap()).unwrap();
        sqlx::query("INSERT INTO channels(id,brand_id,name,provider,encrypted_credentials,connection_identity,connection_status) VALUES($1,$2,'Fixture',$3,$4,$5,'connected') ON CONFLICT(id) DO UPDATE SET encrypted_credentials=$4,connection_status='connected'").bind(id).bind(self.brand).bind(&c.app.provider).bind(encrypted).bind(c.destination_key()).execute(&self.state.db).await.unwrap();
    }
    async fn start(&self) -> (Uuid, String, HeaderMap) {
        let r = connections::start(
            State(self.state.clone()),
            self.auth.clone(),
            self.headers(),
            Path("x".into()),
            Json(json!({"brand_id":self.brand})),
        )
        .await
        .unwrap();
        let cookie = r
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string();
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, cookie.parse().unwrap());
        let v: Value =
            serde_json::from_slice(&to_bytes(r.into_body(), 100000).await.unwrap()).unwrap();
        let state = url::Url::parse(v["data"]["authorization_url"].as_str().unwrap())
            .unwrap()
            .query_pairs()
            .find(|(k, _)| k == "state")
            .unwrap()
            .1
            .to_string();
        (
            v["data"]["flow_id"].as_str().unwrap().parse().unwrap(),
            state,
            h,
        )
    }
    async fn publication(&self, channel: Uuid, session: &str) -> Uuid {
        let post = Uuid::new_v4();
        let id = Uuid::new_v4();
        let media = Uuid::new_v4();
        let storage = media.to_string();
        tokio::fs::write(self.state.config.media_dir.join(&storage), vec![0; 10])
            .await
            .unwrap();
        sqlx::query("INSERT INTO media(id,brand_id,name,mime,size,storage_key) VALUES($1,$2,'fixture.mp4','video/mp4',10,$3)").bind(media).bind(self.brand).bind(storage).execute(&self.state.db).await.unwrap();
        sqlx::query("INSERT INTO posts(id,brand_id,text) VALUES($1,$2,'test')")
            .bind(post)
            .bind(self.brand)
            .execute(&self.state.db)
            .await
            .unwrap();
        sqlx::query("INSERT INTO publications(id,post_id,brand_id,channel_id,post_version,scheduled_at,state,payload) VALUES($1,$2,$3,$4,1,now(),'uncertain',$5)").bind(id).bind(post).bind(self.brand).bind(channel).bind(json!({"text":"test","title":"Test","media_ids":[media],"options":{"privacy":"private","made_for_kids":false}})).execute(&self.state.db).await.unwrap();
        let cp=crypto::encrypt(&[13;32],id,&json!({"stage":"youtube_upload","session":format!("https://www.googleapis.com/{session}"),"offset":0,"size":10}).to_string()).unwrap();
        sqlx::query("INSERT INTO provider_delivery_checkpoints(publication_id,encrypted_checkpoint) VALUES($1,$2)").bind(id).bind(cp).execute(&self.state.db).await.unwrap();
        id
    }
}
