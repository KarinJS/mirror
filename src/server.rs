use crate::config::AppState;
use crate::error::{AppError, AppResult};
use crate::geo::check_geo;
use crate::http_utils::get_client_country;
use crate::routes;
use crate::stats::Stats;
use crate::sync;
use axum::{
    extract::State,
    http::{HeaderMap, Uri},
    middleware,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use http::{header, HeaderValue, Request, StatusCode};
use reqwest::Client;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use tower::{layer::layer_fn, Service, ServiceBuilder};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::{debug, info, Span};

async fn reject_query(req: axum::http::Request<axum::body::Body>, next: middleware::Next) -> impl IntoResponse {
    if req.uri().query().is_some() {
        return StatusCode::NOT_FOUND.into_response();
    }
    next.run(req).await
}

/// Wraps a service to add immutable cache headers for hashed /assets/ files.
#[derive(Clone)]
struct AssetCache<S> {
    inner: S,
}

impl<S, B> Service<Request<axum::body::Body>> for AssetCache<S>
where
    S: Service<Request<axum::body::Body>, Response = Response<B>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    B: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<axum::body::Body>) -> Self::Future {
        let cache = req.uri().path().starts_with("/assets/");
        let mut inner = self.inner.clone();
        Box::pin(async move {
            let mut resp = inner.call(req).await?;
            if cache {
                resp.headers_mut().insert(
                    header::CACHE_CONTROL,
                    HeaderValue::from_static("public, max-age=31536000, immutable"),
                );
            }
            Ok(resp)
        })
    }
}

#[derive(Clone)]
struct AppContext {
    state: AppState,
    stats: Stats,
    client: Client,
}

pub async fn run() -> anyhow::Result<()> {
    let state = AppState::load().await?;
    let stats = Stats::new();
    let client = Client::builder()
        .build()?;

    let sync_client = reqwest::Client::builder().build()?;

    let config = state.config.read().await;
    let host = config.host.clone();
    let port = config.port;
    drop(config);

    let ctx = AppContext {
        state: state.clone(),
        stats: stats.clone(),
        client,
    };

    tokio::spawn(sync::config_sync_task(state, sync_client));

    let access_log = TraceLayer::new_for_http()
        .on_response(|resp: &Response<_>, latency: std::time::Duration, _span: &Span| {
            let status = resp.status().as_u16();
            let ms = latency.as_millis();
            if status >= 500 {
                tracing::warn!("{} {}ms", status, ms);
            } else {
                info!("{} {}ms", status, ms);
            }
        });

    let serve_dir = ServiceBuilder::new()
        .layer(layer_fn(|inner| AssetCache { inner }))
        .service(ServeDir::new("webui"));

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/robots.txt", get(robots))
        .route("/stats", get(stats_handler))
        .route("/gh/{*path}", get(gh_handler))
        .route("/raw/{*path}", get(raw_handler))
        .route("/avatar/{*path}", get(avatar_handler))
        .route("/unpkg/{*path}", get(unpkg_handler))
        .route("/mirror/{*path}", get(mirror_handler))
        .fallback_service(serve_dir)
        .layer(ServiceBuilder::new().layer(access_log))
        .layer(middleware::from_fn(reject_query))
        .with_state(ctx);

    let addr = format!("{}:{}", host, port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    let display_host = if host == "0.0.0.0" { "127.0.0.1" } else { &host };
    let url = format!("http://{}:{}", display_host, port);
    let ver = env!("CARGO_PKG_VERSION");

    println!();
    println!("  \x1b[1;36m◆  mirror.karinjs.com\x1b[0m  v{ver}");
    println!("  │");
    println!("  ├  Local   \x1b[1m{url}\x1b[0m");
    println!("  └  Logs    logs/mirror.log");
    println!();

    debug!("listening on {}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}

async fn healthz() -> &'static str {
    "ok"
}

async fn robots() -> &'static str {
    "User-agent: *\nDisallow: /\n"
}

async fn stats_handler(State(ctx): State<AppContext>) -> impl IntoResponse {
    let snapshot = ctx.stats.snapshot().await;
    axum::Json(snapshot)
}

async fn gh_handler(
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    uri: Uri,
) -> AppResult<Response> {
    ctx.stats.bump("gh").await;
    check_request(&ctx.state, &headers, uri.path()).await?;
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
    ctx.stats.bump("raw").await;
    check_request(&ctx.state, &headers, uri.path()).await?;
    routes::handle_raw(State(ctx.state), State(ctx.client), headers, uri.path().to_string()).await
}

async fn avatar_handler(
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    uri: Uri,
) -> AppResult<Response> {
    ctx.stats.bump("avatar").await;
    check_request(&ctx.state, &headers, uri.path()).await?;
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
    ctx.stats.bump("unpkg").await;
    check_request(&ctx.state, &headers, uri.path()).await?;
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
    ctx.stats.bump("mirror").await;
    check_request(&ctx.state, &headers, uri.path()).await?;
    routes::handle_mirror(
        State(ctx.state),
        State(ctx.client),
        headers,
        uri.path().to_string(),
    )
    .await
}

async fn check_request(state: &AppState, headers: &HeaderMap, _path: &str) -> AppResult<()> {
    let config = state.config.read().await;

    // Check auth header
    if config.auth.enabled {
        let val = headers
            .get(&config.auth.key)
            .and_then(|v| v.to_str().ok());
        if val != Some(&config.auth.value) {
            return Err(AppError::Unauthorized);
        }
    }

    // Check geo
    let country = get_client_country(headers, &config.geo.header_name);
    if !check_geo(&config.geo, country.as_deref()) {
        return Err(AppError::GeoBlocked);
    }

    Ok(())
}
