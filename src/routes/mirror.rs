use crate::config::{AppState, MirrorRule};
use crate::error::{AppError, AppResult};
use crate::proxy::{proxy_upstream, ProxyOptions, CacheTtl};
use crate::validation::{validate_host, validate_path_component, resolve_and_validate_host};
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::Response;
use reqwest::Client;

pub async fn handle_mirror(
    State(state): State<AppState>,
    State(client): State<Client>,
    headers: HeaderMap,
    path: String,
) -> AppResult<Response> {
    // Path format: /mirror/<host>/<path>
    let rest = path.trim_start_matches("/mirror/");
    let parts: Vec<&str> = rest.splitn(2, '/').collect();
    if parts.len() < 2 {
        return Err(AppError::NotFound);
    }

    let host = parts[0];
    let file_path = parts[1];

    // Validate components
    if !validate_host(host) || !validate_path_component(file_path) {
        return Err(AppError::NotFound);
    }

    // Extract hostname and port for DNS rebinding validation
    let (hostname, port) = match host.rfind(':') {
        Some(pos) => (&host[..pos], host[pos+1..].parse::<u16>().unwrap_or(443)),
        None => (host, 443u16),
    };
    if let Err(reason) = resolve_and_validate_host(hostname, port).await {
        tracing::warn!("mirror DNS rebinding blocked for {}: {}", host, reason);
        return Err(AppError::NotFound);
    }

    // Normalize the target URL: lowercase host, ensure consistent path format
    let normalized_host = host.to_ascii_lowercase();
    let target = format!("https://{}/{}", normalized_host, file_path);

    // Look up the rule under a short-lived read lock. The validated `target` is
    // captured by value before the lock is released, so dropping it ahead of the
    // network I/O (below) is safe — there is no use-after-check race.
    let whitelists = state.whitelists.read().await;
    // Try exact match first, then try with normalized host
    let rule = whitelists.mirror.get(&target)
        .or_else(|| {
            // Fallback: try original (case-sensitive) host for backwards compatibility
            let original_target = format!("https://{}/{}", host, file_path);
            if original_target != target {
                whitelists.mirror.get(&original_target)
            } else {
                None
            }
        });

    let (ttl, max_size) = match rule {
        Some(MirrorRule::Simple(t)) => (CacheTtl::from_config(*t), None),
        Some(MirrorRule::Complex { ttl, max_size }) => (CacheTtl::from_config(*ttl), *max_size),
        None => {
            return Err(AppError::NotFound);
        }
    };
    drop(whitelists); // Release lock before network I/O

    proxy_upstream(
        &state,
        &client,
        &headers,
        &target,
        ProxyOptions { ttl, max_size },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validation::validate_host;

    #[test]
    fn test_validate_host() {
        assert!(validate_host("example.com"));
        assert!(validate_host("sub.example.com"));
        assert!(validate_host("example.com:8080"));
        // SSRF: internal IPs are rejected
        assert!(!validate_host("192.168.1.1"));
        assert!(!validate_host("127.0.0.1"));
        assert!(!validate_host(""));
        assert!(!validate_host("example..com"));
        assert!(!validate_host("example//com"));
        assert!(!validate_host("example\\com"));
        assert!(!validate_host("../etc/passwd"));
    }

    #[test]
    fn test_validate_path() {
        assert!(validate_path_component("file.zip"));
        assert!(validate_path_component("path/to/file.zip"));
        assert!(!validate_path_component(""));
        assert!(!validate_path_component("../etc/passwd"));
        assert!(!validate_path_component("path//file"));
    }
}
