use crate::config::AppState;
use crate::error::{AppError, AppResult};
use crate::validation::resolve_and_validate_host;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use futures::stream::StreamExt;
use reqwest::Client;
use std::time::Duration;
use tracing::warn;

/// Strip the scheme (case-insensitive) from a URL, returning the part after `://`.
fn strip_url_scheme(url: &str) -> Option<&str> {
    let lower = url.to_ascii_lowercase();
    if let Some(pos) = lower.find("://") {
        Some(&url[pos + 3..])
    } else {
        None
    }
}

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

/// Type-safe cache TTL strategy. Replaces magic i32 sentinel values
/// (-2=passthrough, -1=immutable, 0=no-store, >0=max-age).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheTtl {
    /// Passthrough upstream cache-control headers (-2)
    Passthrough,
    /// Immutable, 1-year max-age (-1)
    Immutable,
    /// No caching, strip validators (0)
    NoStore,
    /// Explicit max-age in seconds (positive value)
    MaxAge(u32),
}

impl CacheTtl {
    /// Convert from the i32 config value to the enum.
    pub fn from_config(ttl: i32) -> Self {
        match ttl {
            -2 => CacheTtl::Passthrough,
            -1 => CacheTtl::Immutable,
            0 => CacheTtl::NoStore,
            n if n > 0 => CacheTtl::MaxAge(n as u32),
            _ => {
                warn!("invalid ttl value {ttl}, falling back to no-store");
                CacheTtl::NoStore
            }
        }
    }
}

pub struct ProxyOptions {
    pub ttl: CacheTtl,
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
    let retries = config.mirror.fetch_retries;
    drop(config);

    // DNS rebinding protection: resolve hostname and verify all IPs are globally routable.
    // Extract host from target URL (scheme://host:port/path).
    if let Some(after_scheme) = strip_url_scheme(target) {
        let host_port = after_scheme.split('/').next().unwrap_or("");
        let (hostname, port) = match host_port.rfind(':') {
            Some(pos) => (&host_port[..pos], host_port[pos+1..].parse::<u16>().unwrap_or(443)),
            None => (host_port, 443u16),
        };
        if let Err(reason) = resolve_and_validate_host(hostname, port).await {
            warn!("DNS rebinding blocked for {}: {}", target, reason);
            return Err(AppError::BadGateway);
        }
    }

    let mut req_builder = client.get(target).timeout(timeout);

    for header_name in FORWARD_REQ_HEADERS {
        if let Some(value) = req_headers.get(*header_name) {
            req_builder = req_builder.header(*header_name, value);
        }
    }
    req_builder = req_builder.header("accept-encoding", "identity");

    // Build the request once, then retry sends on retryable failures.
    // GET has no body stream, so the request can be safely cloned per attempt.
    let request = req_builder.build().map_err(|e| {
        warn!("upstream request build failed: {} - {}", target, e);
        AppError::BadGateway
    })?;

    let upstream = send_with_retry(client, request, target, retries).await?;

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

    // For upstream error responses, replace the body with a generic message
    // to prevent leaking internal details from upstream error pages.
    if status.is_server_error() {
        out_headers.insert("content-type", HeaderValue::from_static("application/json"));
        let error_body = serde_json::json!({ "error": "upstream_error" });
        return Ok((status, out_headers, axum::body::Body::from(error_body.to_string())).into_response());
    }

    apply_ttl(&mut out_headers, options.ttl, upstream.headers());

    // Security headers to prevent MIME type sniffing
    out_headers.insert("x-content-type-options", HeaderValue::from_static("nosniff"));

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

/// Send an upstream request with exponential backoff retry.
///
/// Only retries failures that are safe to retry on an idempotent GET, and only
/// within the window before response headers arrive (the body has not yet
/// started streaming):
///   - connection / request errors (network, non-business reqwest errors)
///   - timeouts
///   - upstream 5xx responses (502/503/504 etc.)
///
/// Does NOT retry 4xx responses. Backoff is a fixed exponential schedule
/// (~200ms, ~400ms, ...) with no random jitter. Total attempts = retries + 1.
/// Whether an upstream response with this status should be retried on an
/// idempotent GET. Only 5xx server errors are retryable; 4xx (and any non-error
/// status) are returned to the client as-is.
fn is_retryable_status(status: StatusCode) -> bool {
    status.is_server_error()
}

async fn send_with_retry(
    client: &Client,
    request: reqwest::Request,
    target: &str,
    retries: u32,
) -> AppResult<reqwest::Response> {
    let mut attempt = 0u32;
    loop {
        // Clone the request for this attempt so the original survives for a retry.
        // GET requests have no streaming body, so try_clone always succeeds here;
        // if cloning is somehow unavailable we cannot retry, so send the original.
        let this_req = match request.try_clone() {
            Some(cloned) => cloned,
            None => {
                // Cannot clone (body stream) — send the original without retry.
                return send_once(client, request, target).await;
            }
        };

        match send_once(client, this_req, target).await {
            Ok(resp) if is_retryable_status(resp.status()) && attempt < retries => {
                let status = resp.status();
                attempt += 1;
                let backoff = Duration::from_millis(200u64 * (1u64 << (attempt - 1)));
                warn!(
                    "upstream retry {}/{} (status {}) for {}, backing off {:?}",
                    attempt, retries, status, target, backoff
                );
                tokio::time::sleep(backoff).await;
                continue;
            }
            Ok(resp) => return Ok(resp),
            Err(err) if attempt < retries => {
                attempt += 1;
                let backoff = Duration::from_millis(200u64 * (1u64 << (attempt - 1)));
                warn!(
                    "upstream retry {}/{} ({}) for {}, backing off {:?}",
                    attempt, retries, err, target, backoff
                );
                tokio::time::sleep(backoff).await;
                continue;
            }
            Err(err) => return Err(err),
        }
    }
}

/// Perform a single upstream send, mapping reqwest errors to AppError.
async fn send_once(
    client: &Client,
    request: reqwest::Request,
    target: &str,
) -> AppResult<reqwest::Response> {
    client.execute(request).await.map_err(|e| {
        warn!("upstream fetch failed: {} - {}", target, e);
        if e.is_timeout() {
            AppError::GatewayTimeout
        } else {
            AppError::BadGateway
        }
    })
}

pub async fn resolve_upstream_url(
    client: &Client,
    req_headers: &HeaderMap,
    target: &str,
    timeout: Duration,
) -> AppResult<String> {
    // DNS rebinding protection for resolve requests too
    if let Some(after_scheme) = strip_url_scheme(target) {
        let host_port = after_scheme.split('/').next().unwrap_or("");
        let (hostname, port) = match host_port.rfind(':') {
            Some(pos) => (&host_port[..pos], host_port[pos+1..].parse::<u16>().unwrap_or(443)),
            None => (host_port, 443u16),
        };
        if let Err(reason) = resolve_and_validate_host(hostname, port).await {
            warn!("DNS rebinding blocked for {}: {}", target, reason);
            return Err(AppError::BadGateway);
        }
    }

    let mut req_builder = client.head(target).timeout(timeout);
    for header_name in FORWARD_REQ_HEADERS {
        if let Some(value) = req_headers.get(*header_name) {
            req_builder = req_builder.header(*header_name, value);
        }
    }

    let upstream = req_builder.send().await.map_err(|e| {
        warn!("upstream resolve failed: {} - {}", target, e);
        if e.is_timeout() {
            AppError::GatewayTimeout
        } else {
            AppError::BadGateway
        }
    })?;

    // Some servers don't support HEAD; fall back to GET to resolve the final URL.
    // We only need the URL after redirects, not the body — use a range request
    // to minimize data transfer, then drop the response immediately.
    if upstream.status() == StatusCode::METHOD_NOT_ALLOWED {
        let mut get_req = client.get(target).timeout(timeout);
        for header_name in FORWARD_REQ_HEADERS {
            if let Some(value) = req_headers.get(*header_name) {
                get_req = get_req.header(*header_name, value);
            }
        }
        // Request only 1 byte to minimize data transfer
        get_req = get_req.header("range", "bytes=0-0");
        let resp = get_req.send().await.map_err(|e| {
            warn!("upstream resolve (GET) failed: {} - {}", target, e);
            if e.is_timeout() {
                AppError::GatewayTimeout
            } else {
                AppError::BadGateway
            }
        })?;
        let final_url = resp.url().to_string();
        drop(resp); // abort the body stream immediately
        return Ok(final_url);
    }

    // response.url() is the final URL after reqwest has followed all redirects
    Ok(upstream.url().to_string())
}

fn apply_ttl(headers: &mut HeaderMap, ttl: CacheTtl, upstream_headers: &HeaderMap) {
    match ttl {
        CacheTtl::Passthrough => {
            if let Some(cc) = upstream_headers.get("cache-control") {
                headers.insert("cache-control", cc.clone());
            }
            if let Some(etag) = upstream_headers.get("etag") {
                headers.insert("etag", etag.clone());
            }
        }
        CacheTtl::Immutable => {
            headers.insert(
                "cache-control",
                HeaderValue::from_static("public, max-age=31536000, immutable"),
            );
        }
        CacheTtl::NoStore => {
            headers.insert("cache-control", HeaderValue::from_static("no-store"));
            headers.remove("etag");
            headers.remove("last-modified");
        }
        CacheTtl::MaxAge(secs) => {
            if let Ok(val) = HeaderValue::from_str(&format!("public, max-age={secs}")) {
                headers.insert("cache-control", val);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_ttl_immutable() {
        let mut headers = HeaderMap::new();
        let upstream = HeaderMap::new();
        apply_ttl(&mut headers, CacheTtl::Immutable, &upstream);
        assert_eq!(
            headers.get("cache-control").unwrap(),
            "public, max-age=31536000, immutable"
        );
    }

    #[test]
    fn test_apply_ttl_no_store_strips_validators() {
        let mut headers = HeaderMap::new();
        headers.insert("etag", HeaderValue::from_static("\"abc123\""));
        headers.insert(
            "last-modified",
            HeaderValue::from_static("Thu, 01 Jan 2026 00:00:00 GMT"),
        );
        let upstream = HeaderMap::new();
        apply_ttl(&mut headers, CacheTtl::NoStore, &upstream);
        assert_eq!(headers.get("cache-control").unwrap(), "no-store");
        assert!(headers.get("etag").is_none(), "etag must be stripped");
        assert!(
            headers.get("last-modified").is_none(),
            "last-modified must be stripped"
        );
    }

    #[test]
    fn test_apply_ttl_positive() {
        let mut headers = HeaderMap::new();
        let upstream = HeaderMap::new();
        apply_ttl(&mut headers, CacheTtl::MaxAge(300), &upstream);
        assert_eq!(headers.get("cache-control").unwrap(), "public, max-age=300");
    }

    #[test]
    fn test_apply_ttl_passthrough_from_upstream() {
        let mut headers = HeaderMap::new();
        let mut upstream = HeaderMap::new();
        upstream.insert(
            "cache-control",
            HeaderValue::from_static("max-age=3600, public"),
        );
        upstream.insert("etag", HeaderValue::from_static("\"xyz\""));
        apply_ttl(&mut headers, CacheTtl::Passthrough, &upstream);
        assert_eq!(
            headers.get("cache-control").unwrap(),
            "max-age=3600, public"
        );
        assert_eq!(headers.get("etag").unwrap(), "\"xyz\"");
    }

    #[test]
    fn test_apply_ttl_passthrough_missing_upstream_headers() {
        let mut headers = HeaderMap::new();
        let upstream = HeaderMap::new(); // no cache-control or etag
        apply_ttl(&mut headers, CacheTtl::Passthrough, &upstream);
        assert!(headers.get("cache-control").is_none());
        assert!(headers.get("etag").is_none());
    }

    #[test]
    fn test_cache_ttl_from_config() {
        assert_eq!(CacheTtl::from_config(-2), CacheTtl::Passthrough);
        assert_eq!(CacheTtl::from_config(-1), CacheTtl::Immutable);
        assert_eq!(CacheTtl::from_config(0), CacheTtl::NoStore);
        assert_eq!(CacheTtl::from_config(300), CacheTtl::MaxAge(300));
        assert_eq!(CacheTtl::from_config(86400), CacheTtl::MaxAge(86400));
    }

    #[test]
    fn test_cache_ttl_from_config_invalid_negative_falls_back() {
        // Invalid negative values fall back to NoStore
        assert_eq!(CacheTtl::from_config(-5), CacheTtl::NoStore);
        assert_eq!(CacheTtl::from_config(-100), CacheTtl::NoStore);
    }

    // ── is_retryable_status ──────────────────────────────────────────────

    #[test]
    fn test_is_retryable_status_5xx() {
        assert!(is_retryable_status(StatusCode::INTERNAL_SERVER_ERROR));
        assert!(is_retryable_status(StatusCode::BAD_GATEWAY));
        assert!(is_retryable_status(StatusCode::SERVICE_UNAVAILABLE));
        assert!(is_retryable_status(StatusCode::GATEWAY_TIMEOUT));
    }

    #[test]
    fn test_is_retryable_status_4xx_not_retryable() {
        assert!(!is_retryable_status(StatusCode::NOT_FOUND));
        assert!(!is_retryable_status(StatusCode::FORBIDDEN));
        assert!(!is_retryable_status(StatusCode::TOO_MANY_REQUESTS));
        assert!(!is_retryable_status(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn test_is_retryable_status_2xx_3xx_not_retryable() {
        assert!(!is_retryable_status(StatusCode::OK));
        assert!(!is_retryable_status(StatusCode::PARTIAL_CONTENT));
        assert!(!is_retryable_status(StatusCode::NOT_MODIFIED));
        assert!(!is_retryable_status(StatusCode::FOUND));
    }

    // ── strip_url_scheme ─────────────────────────────────────────────────

    #[test]
    fn test_strip_url_scheme() {
        assert_eq!(
            strip_url_scheme("https://github.com/foo.png"),
            Some("github.com/foo.png")
        );
        assert_eq!(
            strip_url_scheme("HTTP://Example.com/x"),
            Some("Example.com/x")
        );
        assert_eq!(strip_url_scheme("no-scheme/path"), None);
    }
}
