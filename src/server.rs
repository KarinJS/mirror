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
use http::StatusCode;
use reqwest::Client;
use tower::ServiceBuilder;
#[cfg(debug_assertions)]
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::{debug, info, Span};

async fn reject_query(
    req: axum::http::Request<axum::body::Body>,
    next: middleware::Next,
) -> impl IntoResponse {
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
}

pub async fn run() -> anyhow::Result<()> {
    let state = AppState::load().await?;
    let stats = Stats::new();
    let client = Client::builder().build()?;

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

    let access_log = TraceLayer::new_for_http().on_response(
        |resp: &Response<_>, latency: std::time::Duration, _span: &Span| {
            let status = resp.status().as_u16();
            let ms = latency.as_millis();
            if status >= 500 {
                tracing::warn!("{} {}ms", status, ms);
            } else {
                info!("{} {}ms", status, ms);
            }
        },
    );

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
        .with_state(ctx);

    #[cfg(debug_assertions)]
    let app = app.fallback_service(ServeDir::new("webui"));

    #[cfg(not(debug_assertions))]
    let app = app.fallback(static_handler);

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
    routes::handle_raw(
        State(ctx.state),
        State(ctx.client),
        headers,
        uri.path().to_string(),
    )
    .await
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
        let val = headers.get(&config.auth.key).and_then(|v| v.to_str().ok());
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
