use crate::config::AppState;
use crate::error::{AppError, AppResult};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use futures::stream::StreamExt;
use reqwest::Client;
use std::time::Duration;
use tracing::warn;

const FORWARD_REQ_HEADERS: &[&str] = &[
    "range",
    "if-none-match",
    "if-modified-since",
    "accept",
    "user-agent",
];

const FORWARD_RES_HEADERS: &[&str] = &[
    "content-type",
    "content-range",
    "accept-ranges",
    "last-modified",
    "etag",
    "content-disposition",
];

pub struct ProxyOptions {
    pub ttl: i32,
    pub max_size: Option<usize>,
}

pub async fn proxy_upstream(
    state: &AppState,
    client: &Client,
    req_headers: &HeaderMap,
    target: &str,
    options: ProxyOptions,
) -> AppResult<Response> {
    let config = state.config.read().await;
    let limit = options
        .max_size
        .unwrap_or(config.mirror.default_max_size)
        .min(config.mirror.absolute_max_size);

    let timeout = Duration::from_millis(config.mirror.fetch_timeout_ms);
    drop(config);

    let mut req_builder = client.get(target).timeout(timeout);

    for header_name in FORWARD_REQ_HEADERS {
        if let Some(value) = req_headers.get(*header_name) {
            req_builder = req_builder.header(*header_name, value);
        }
    }
    req_builder = req_builder.header("accept-encoding", "identity");

    let upstream = req_builder
        .send()
        .await
        .map_err(|e| {
            warn!("upstream fetch failed: {} - {}", target, e);
            if e.is_timeout() {
                AppError::GatewayTimeout
            } else {
                AppError::BadGateway
            }
        })?;

    if let Some(cl) = upstream.headers().get("content-length") {
        if let Ok(size_str) = cl.to_str() {
            if let Ok(size) = size_str.parse::<usize>() {
                if size > limit {
                    return Err(AppError::PayloadTooLarge);
                }
            }
        }
    }

    let status = upstream.status();
    let mut out_headers = HeaderMap::new();

    for header_name in FORWARD_RES_HEADERS {
        if let Some(value) = upstream.headers().get(*header_name) {
            out_headers.insert(*header_name, value.clone());
        }
    }

    apply_ttl(&mut out_headers, options.ttl, upstream.headers());

    let body_stream = upstream.bytes_stream();
    let limited_stream = body_stream.scan(0usize, move |total, chunk| {
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => {
                warn!("stream error: {}", e);
                return futures::future::ready(Some(Err(AppError::BadGateway)));
            }
        };

        *total += chunk.len();
        if *total > limit {
            return futures::future::ready(Some(Err(AppError::PayloadTooLarge)));
        }

        futures::future::ready(Some(Ok(chunk)))
    });

    let body = axum::body::Body::from_stream(limited_stream);
    Ok((status, out_headers, body).into_response())
}

pub async fn resolve_upstream_url(
    client: &Client,
    req_headers: &HeaderMap,
    target: &str,
    timeout: Duration,
) -> AppResult<String> {
    let mut current = target.to_string();

    for _ in 0..10 {
        let mut req_builder = client.head(&current).timeout(timeout);

        for header_name in FORWARD_REQ_HEADERS {
            if let Some(value) = req_headers.get(*header_name) {
                req_builder = req_builder.header(*header_name, value);
            }
        }

        let upstream = req_builder.send().await.map_err(|e| {
            warn!("upstream resolve failed: {} - {}", current, e);
            if e.is_timeout() {
                AppError::GatewayTimeout
            } else {
                AppError::BadGateway
            }
        })?;

        if upstream.status() == StatusCode::METHOD_NOT_ALLOWED {
            let mut get_req = client.get(&current).timeout(timeout);
            for header_name in FORWARD_REQ_HEADERS {
                if let Some(value) = req_headers.get(*header_name) {
                    get_req = get_req.header(*header_name, value);
                }
            }
            let upstream = get_req.send().await.map_err(|e| {
                warn!("upstream resolve (GET) failed: {} - {}", current, e);
                if e.is_timeout() {
                    AppError::GatewayTimeout
                } else {
                    AppError::BadGateway
                }
            })?;

            if let Some(location) = upstream.headers().get("location") {
                if is_redirect_status(upstream.status()) {
                    current = location.to_str().unwrap_or(&current).to_string();
                    continue;
                }
            }
            return Ok(current);
        }

        if let Some(location) = upstream.headers().get("location") {
            if is_redirect_status(upstream.status()) {
                current = location.to_str().unwrap_or(&current).to_string();
                continue;
            }
        }

        return Ok(current);
    }

    warn!("upstream redirect loop: {}", target);
    Err(AppError::BadGateway)
}

fn apply_ttl(headers: &mut HeaderMap, ttl: i32, upstream_headers: &HeaderMap) {
    if ttl == -2 {
        if let Some(cc) = upstream_headers.get("cache-control") {
            headers.insert("cache-control", cc.clone());
        }
        if let Some(etag) = upstream_headers.get("etag") {
            headers.insert("etag", etag.clone());
        }
        return;
    }
    if ttl == -1 {
        headers.insert(
            "cache-control",
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        );
        return;
    }
    if ttl == 0 {
        headers.insert("cache-control", HeaderValue::from_static("no-store"));
        headers.remove("etag");
        headers.remove("last-modified");
        return;
    }
    headers.insert(
        "cache-control",
        HeaderValue::from_str(&format!("public, max-age={}", ttl)).unwrap(),
    );
}

fn is_redirect_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::MOVED_PERMANENTLY
            | StatusCode::FOUND
            | StatusCode::SEE_OTHER
            | StatusCode::TEMPORARY_REDIRECT
            | StatusCode::PERMANENT_REDIRECT
    )
}
