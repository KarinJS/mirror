use crate::config::AppState;
use crate::error::{AppError, AppResult};
use crate::geo::check_geo;
use crate::http_utils::get_client_country;
use crate::origin_acl;
use crate::routes;
use crate::stats::{RouteBucket, Stats};
use crate::sync;
use axum::{
    extract::State,
    http::{HeaderMap, Uri},
    middleware,
    response::{IntoResponse, Response},
    routing::get,
    Router,
    extract::DefaultBodyLimit,
};
use dashmap::DashMap;
use http::{Request, StatusCode};
use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceBuilder;
#[cfg(debug_assertions)]
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::{debug, info, Span};

const MAX_REQUEST_BODY_BYTES: usize = 1024 * 1024; // 1 MB — used by DefaultBodyLimit

async fn reject_query(req: axum::http::Request<axum::body::Body>, next: middleware::Next) -> impl IntoResponse {
    if req.uri().query().is_some() {
        return StatusCode::NOT_FOUND.into_response();
    }
    next.run(req).await
}

#[derive(Clone)]
struct AppContext {
    state: AppState,
    stats: Stats,
    client: Client,
    rate_limiter: RateLimiter,
}

/// Simple sliding-window rate limiter: tracks requests per IP over a time window.
///
/// Backed by a `DashMap` so each request only locks a single shard instead of a
/// global mutex. Stale entries are reclaimed by a background sweep task (see
/// `RateLimiter::spawn_cleanup`) rather than on the request path.
#[derive(Clone)]
struct RateLimiter {
    inner: Arc<DashMap<String, (u64, std::time::Instant)>>,
    max_requests: u64,
    window: Duration,
}

impl RateLimiter {
    fn new(max_requests: u64, window_secs: u64) -> Self {
        Self {
            inner: Arc::new(DashMap::new()),
            max_requests,
            window: Duration::from_secs(window_secs),
        }
    }

    /// Check if the request should be allowed. Returns true if allowed, false if rate limited.
    fn check(&self, key: &str) -> bool {
        let now = std::time::Instant::now();

        // Only the shard for `key` is locked, not the whole map.
        let mut entry = self.inner.entry(key.to_string()).or_insert((0, now));

        if now.duration_since(entry.1) >= self.window {
            // Window expired, reset
            *entry = (1, now);
            return true;
        }

        if entry.0 >= self.max_requests {
            return false;
        }

        entry.0 += 1;
        true
    }

    /// Spawn a background task that periodically evicts entries whose window has
    /// fully expired, keeping memory bounded without touching the request path.
    fn spawn_cleanup(&self) {
        let inner = self.inner.clone();
        let window = self.window;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                let now = std::time::Instant::now();
                inner.retain(|_, (_, ts)| now.duration_since(*ts) < window);
            }
        });
    }
}


pub async fn run() -> anyhow::Result<()> {
    let state = AppState::load().await?;
    let stats = Stats::new();
    let client = Client::builder()
        .pool_max_idle_per_host(64)
        .pool_idle_timeout(Duration::from_secs(90))
        .build()?;

    let sync_client = reqwest::Client::builder().build()?;
    let origin_acl_client = reqwest::Client::builder().build()?;

    let config = state.config.read().await;
    let host = config.host.clone();
    let port = config.port;
    drop(config);

    // Validate auth config at startup
    {
        let config = state.config.read().await;
        if config.auth.enabled {
            if config.auth.key.is_empty() {
                anyhow::bail!("auth.enabled is true but auth.key is empty; all requests would be rejected");
            }
            if config.auth.value.is_empty() {
                anyhow::bail!("auth.enabled is true but auth.value is empty");
            }
            // Validate the key is a syntactically valid HTTP header name
            if http::header::HeaderName::from_bytes(config.auth.key.as_bytes()).is_err() {
                anyhow::bail!("auth.key {:?} is not a valid HTTP header name", config.auth.key);
            }
        }
    }

    // Rate limiter: 120 requests per 60 seconds per IP
    let rate_limiter = RateLimiter::new(120, 60);
    // Periodically reclaim stale entries off the request path.
    rate_limiter.spawn_cleanup();

    let ctx = AppContext {
        state: state.clone(),
        stats: stats.clone(),
        client,
        rate_limiter,
    };

    let app = build_router(ctx);

    let sync_handle = tokio::spawn(sync::config_sync_task(state.clone(), sync_client));
    // Monitor the sync task — if it panics, log and restart
    tokio::spawn(async move {
        match sync_handle.await {
            Ok(()) => tracing::warn!("config sync task exited unexpectedly"),
            Err(e) if e.is_panic() => {
                tracing::error!("config sync task panicked: {:?}", e.into_panic());
            }
            Err(e) => tracing::error!("config sync task join error: {e}"),
        }
    });

    // EO origin-protection auto-pull (off unless originProtection.enabled).
    let acl_handle = tokio::spawn(origin_acl::origin_protection_task(state.clone(), origin_acl_client));
    tokio::spawn(async move {
        match acl_handle.await {
            Ok(()) => tracing::warn!("origin protection task exited unexpectedly"),
            Err(e) if e.is_panic() => {
                tracing::error!("origin protection task panicked: {:?}", e.into_panic());
            }
            Err(e) => tracing::error!("origin protection task join error: {e}"),
        }
    });

    let addr = format!("{}:{}", host, port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    let display_host = if host == "0.0.0.0" {
        "127.0.0.1"
    } else {
        &host
    };
    let url = format!("http://{}:{}", display_host, port);
    let ver = env!("CARGO_PKG_VERSION");

    println!();
    println!("  \x1b[1;36m◆  mirror.karinjs.com\x1b[0m  v{ver}");
    println!("  │");
    println!("  ├  Local   \x1b[1m{url}\x1b[0m");
    println!("  └  Logs    logs/mirror.log");
    println!();

    debug!("binding to {}", addr);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

/// Build the application router. Extracted from `run()` so both the live server
/// and tests can construct an identical router from an `AppContext`.
fn build_router(ctx: AppContext) -> Router {
    let access_log = TraceLayer::new_for_http()
        .on_request(|req: &Request<_>, _span: &Span| {
            debug!("{} {}", req.method(), req.uri());
        })
        .on_response(|resp: &Response<_>, latency: std::time::Duration, _span: &Span| {
            let status = resp.status().as_u16();
            let ms = latency.as_millis();
            if status >= 500 {
                tracing::warn!("{} {}ms", status, ms);
            } else if status >= 400 {
                info!("{} {}ms", status, ms);
            } else {
                debug!("{} {}ms", status, ms);
            }
        },
    );

    // /healthz and /robots.txt intentionally bypass auth/geo middleware
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/robots.txt", get(robots))
        .route("/stats", get(stats_handler))
        .route("/gh/{*path}", get(gh_handler))
        .route("/raw/{*path}", get(raw_handler))
        .route("/avatar/{*path}", get(avatar_handler))
        .route("/unpkg/{*path}", get(unpkg_handler))
        .route("/mirror/{*path}", get(mirror_handler))
        .layer(ServiceBuilder::new().layer(access_log))
        .layer(middleware::from_fn(reject_query))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES))
        .with_state(ctx);

    #[cfg(debug_assertions)]
    let app = app.fallback_service(ServeDir::new("webui"));

    #[cfg(not(debug_assertions))]
    let app = app.fallback(static_handler);

    app
}

async fn healthz(State(ctx): State<AppContext>) -> impl IntoResponse {
    let config_loaded = ctx.state.config.try_read().is_ok();
    if config_loaded {
        (StatusCode::OK, "ok")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "config unavailable")
    }
}

async fn robots() -> &'static str {
    "User-agent: *\nDisallow: /\n"
}

async fn stats_handler(
    State(ctx): State<AppContext>,
    headers: HeaderMap,
) -> AppResult<impl IntoResponse> {
    // Protect stats endpoint with the same auth check when auth is enabled
    check_request(&ctx, &headers).await?;
    let snapshot = ctx.stats.snapshot();
    Ok(axum::Json(snapshot))
}

async fn gh_handler(
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    uri: Uri,
) -> AppResult<Response> {
    ctx.stats.bump(RouteBucket::Gh);
    check_request(&ctx, &headers).await?;
    routes::handle_releases(
        State(ctx.state),
        State(ctx.client),
        headers,
        uri.path().to_string(),
    )
    .await
}

async fn raw_handler(
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    uri: Uri,
) -> AppResult<Response> {
    ctx.stats.bump(RouteBucket::Raw);
    check_request(&ctx, &headers).await?;
    routes::handle_raw(State(ctx.state), State(ctx.client), headers, uri.path().to_string()).await
}

async fn avatar_handler(
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    uri: Uri,
) -> AppResult<Response> {
    ctx.stats.bump(RouteBucket::Avatar);
    check_request(&ctx, &headers).await?;
    routes::handle_avatar(
        State(ctx.state),
        State(ctx.client),
        headers,
        uri.path().to_string(),
    )
    .await
}

async fn unpkg_handler(
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    uri: Uri,
) -> AppResult<Response> {
    ctx.stats.bump(RouteBucket::Unpkg);
    check_request(&ctx, &headers).await?;
    routes::handle_unpkg(
        State(ctx.state),
        State(ctx.client),
        headers,
        uri.path().to_string(),
    )
    .await
}

async fn mirror_handler(
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    uri: Uri,
) -> AppResult<Response> {
    ctx.stats.bump(RouteBucket::Mirror);
    check_request(&ctx, &headers).await?;
    routes::handle_mirror(
        State(ctx.state),
        State(ctx.client),
        headers,
        uri.path().to_string(),
    )
    .await
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("shutdown signal received, draining connections...");
}

/// Constant-time string comparison to prevent timing attacks on auth tokens.
/// Processes all bytes regardless of length to avoid leaking length information.
fn constant_time_eq(a: &str, b: &str) -> bool {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    // XOR the lengths first so mismatched lengths don't short-circuit
    let mut result = (a_bytes.len() ^ b_bytes.len()) as u8;
    // Iterate over the longer length, using 0 for out-of-bounds on the shorter
    let max_len = a_bytes.len().max(b_bytes.len());
    for i in 0..max_len {
        let a_byte = if i < a_bytes.len() { a_bytes[i] } else { 0 };
        let b_byte = if i < b_bytes.len() { b_bytes[i] } else { 0 };
        result |= a_byte ^ b_byte;
    }
    result == 0
}

fn extract_client_ip(headers: &HeaderMap, trust_proxy: bool) -> String {
    if !trust_proxy {
        return "direct".to_string();
    }
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

async fn check_request(ctx: &AppContext, headers: &HeaderMap) -> AppResult<()> {
    let config = ctx.state.config.read().await;

    // Rate limit check
    let ip = extract_client_ip(headers, config.trust_proxy_headers);
    if !ctx.rate_limiter.check(&ip) {
        return Err(AppError::RateLimited);
    }

    // Check auth header (constant-time comparison)
    if config.auth.enabled {
        let val = headers
            .get(&config.auth.key)
            .and_then(|v| v.to_str().ok());
        match val {
            Some(v) if constant_time_eq(v, &config.auth.value) => {}
            _ => return Err(AppError::Unauthorized),
        }
    }

    // Check geo
    let country = get_client_country(headers, &config.geo.header_name);
    if !check_geo(&config.geo, country.as_deref()) {
        return Err(AppError::GeoBlocked);
    }

    Ok(())
}

#[cfg(not(debug_assertions))]
async fn static_handler(uri: Uri) -> impl IntoResponse {
    use http::header;
    use rust_embed::RustEmbed;
    #[derive(RustEmbed)]
    #[folder = "webui/dist"]
    struct Assets;

    let path = uri
        .path()
        .trim_start_matches('/')
        .strip_prefix("webui/")
        .unwrap_or_else(|| uri.path().trim_start_matches('/'));

    let path = if path.is_empty() { "index.html" } else { path };

    match Assets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();

            ([(header::CONTENT_TYPE, mime.as_ref())], content.data).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{HeaderValue, Request};
    use tower::ServiceExt;

    // ── constant_time_eq ─────────────────────────────────────────────────

    #[test]
    fn test_constant_time_eq_equal() {
        assert!(constant_time_eq("secret", "secret"));
    }

    #[test]
    fn test_constant_time_eq_not_equal_same_len() {
        assert!(!constant_time_eq("secret", "secref"));
    }

    #[test]
    fn test_constant_time_eq_different_len() {
        assert!(!constant_time_eq("short", "longer-value"));
    }

    #[test]
    fn test_constant_time_eq_empty_vs_empty() {
        assert!(constant_time_eq("", ""));
    }

    #[test]
    fn test_constant_time_eq_prefix_relationship() {
        // A prefix relationship must not be treated as equal.
        assert!(!constant_time_eq("abc", "abcd"));
        assert!(!constant_time_eq("abcd", "abc"));
    }

    // ── extract_client_ip ────────────────────────────────────────────────

    #[test]
    fn test_extract_client_ip_trust_proxy_false() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("1.2.3.4"));
        // When proxy headers are not trusted, the source is always "direct".
        assert_eq!(extract_client_ip(&headers, false), "direct");
    }

    #[test]
    fn test_extract_client_ip_xff_multiple_takes_first_trimmed() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("1.2.3.4, 5.6.7.8"),
        );
        assert_eq!(extract_client_ip(&headers, true), "1.2.3.4");
    }

    #[test]
    fn test_extract_client_ip_real_ip_fallback() {
        let mut headers = HeaderMap::new();
        headers.insert("x-real-ip", HeaderValue::from_static("9.9.9.9"));
        assert_eq!(extract_client_ip(&headers, true), "9.9.9.9");
    }

    #[test]
    fn test_extract_client_ip_xff_preferred_over_real_ip() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("1.1.1.1"));
        headers.insert("x-real-ip", HeaderValue::from_static("9.9.9.9"));
        assert_eq!(extract_client_ip(&headers, true), "1.1.1.1");
    }

    #[test]
    fn test_extract_client_ip_none_present() {
        let headers = HeaderMap::new();
        assert_eq!(extract_client_ip(&headers, true), "unknown");
    }

    // ── RateLimiter ──────────────────────────────────────────────────────

    #[test]
    fn test_rate_limiter_allows_up_to_max_then_rejects() {
        let limiter = RateLimiter::new(3, 60);
        // First 3 requests allowed, 4th rejected.
        assert!(limiter.check("ip-a"));
        assert!(limiter.check("ip-a"));
        assert!(limiter.check("ip-a"));
        assert!(!limiter.check("ip-a"));
    }

    #[test]
    fn test_rate_limiter_keys_independent() {
        let limiter = RateLimiter::new(2, 60);
        assert!(limiter.check("ip-a"));
        assert!(limiter.check("ip-a"));
        assert!(!limiter.check("ip-a"));
        // A different key has its own independent budget.
        assert!(limiter.check("ip-b"));
        assert!(limiter.check("ip-b"));
        assert!(!limiter.check("ip-b"));
    }

    #[tokio::test]
    async fn test_rate_limiter_window_reset() {
        // 1-second window so we can verify reset quickly without a long sleep.
        let limiter = RateLimiter::new(1, 1);
        assert!(limiter.check("ip-c"));
        assert!(!limiter.check("ip-c"), "second request within window rejected");
        // Wait for the window to expire, then the counter resets.
        tokio::time::sleep(Duration::from_millis(1100)).await;
        assert!(limiter.check("ip-c"), "request after window reset allowed");
    }

    // ── router integration (offline paths) ───────────────────────────────

    /// Build an `AppContext` for tests with the given avatar whitelist.
    /// Uses a permissive rate limiter so request-flow tests are not throttled.
    fn test_ctx(avatar_whitelist: Vec<String>) -> AppContext {
        let config_json = r#"{
            "host": "127.0.0.1",
            "port": 7878,
            "publicOrigin": "https://example.com",
            "trustProxyHeaders": false,
            "logLevel": "info",
            "geo": { "mode": "off", "headerName": "EO-Client-IPCountry", "countries": ["CN"] },
            "cacheTTL": { "raw": 300, "avatar": 86400, "unpkg": 300 },
            "mirror": {
                "defaultTTL": 0,
                "defaultMaxSize": 52428800,
                "absoluteMaxSize": 1073741824,
                "fetchTimeoutMs": 30000,
                "fetchRetries": 2
            },
            "cors": { "enabledRoutes": ["raw", "unpkg", "mirror"] },
            "auth": { "enabled": false, "key": "", "value": "" },
            "configSync": { "enabled": false, "intervalSeconds": 300, "url": "" }
        }"#;
        let config: crate::config::AppConfig = serde_json::from_str(config_json).unwrap();
        let state = AppState {
            config: Arc::new(tokio::sync::RwLock::new(config)),
            whitelists: Arc::new(tokio::sync::RwLock::new(crate::config::Whitelists {
                avatar: avatar_whitelist,
                raw: Default::default(),
                releases: Default::default(),
                unpkg: Default::default(),
                mirror: Default::default(),
            })),
        };
        AppContext {
            state,
            stats: Stats::new(),
            client: Client::builder().build().unwrap(),
            rate_limiter: RateLimiter::new(10_000, 60),
        }
    }

    #[tokio::test]
    async fn test_healthz_ok() {
        let app = build_router(test_ctx(vec![]));
        let resp = app
            .oneshot(Request::builder().uri("/healthz").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&body[..], b"ok");
    }

    #[tokio::test]
    async fn test_robots_disallow() {
        let app = build_router(test_ctx(vec![]));
        let resp = app
            .oneshot(Request::builder().uri("/robots.txt").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("Disallow: /"), "robots.txt must disallow: {text}");
    }

    #[tokio::test]
    async fn test_query_string_rejected_by_middleware() {
        // The reject_query middleware 404s any request carrying a query string,
        // including /healthz (the middleware wraps the whole router).
        let app = build_router(test_ctx(vec![]));
        let resp = app
            .oneshot(Request::builder().uri("/healthz?x=1").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_avatar_not_in_whitelist_rejected_before_upstream() {
        // "stranger" is not whitelisted, so the handler returns NotFound before
        // ever attempting an upstream request — no network needed.
        let app = build_router(test_ctx(vec!["karinjs".to_string()]));
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/avatar/stranger.png")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_avatar_invalid_username_rejected() {
        // Invalid username (underscore) is rejected by validation before upstream.
        let app = build_router(test_ctx(vec!["karinjs".to_string()]));
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/avatar/bad_name.png")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
