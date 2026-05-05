use crate::config::{AppState, MirrorRule};
use crate::error::{AppError, AppResult};
use crate::proxy::{proxy_upstream, ProxyOptions};
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::Response;
use reqwest::Client;

fn validate_host(s: &str) -> bool {
    !s.is_empty()
        && !s.contains("//")
        && !s.contains('\\')
        && !s.contains("..")
        && s.chars().all(|c| c.is_alphanumeric() || c == '.' || c == '-' || c == ':')
}

fn validate_path(s: &str) -> bool {
    !s.is_empty() && !s.contains("..") && !s.contains("//") && !s.contains('\\')
}

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
    if !validate_host(host) || !validate_path(file_path) {
        return Err(AppError::NotFound);
    }

    let target = format!("https://{}/{}", host, file_path);

    let whitelists = state.whitelists.read().await;
    let rule = whitelists.mirror.get(&target);

    let (ttl, max_size) = match rule {
        Some(MirrorRule::Simple(t)) => (*t, None),
        Some(MirrorRule::Complex { ttl, max_size }) => (*ttl, *max_size),
        None => {
            return Err(AppError::NotFound);
        }
    };

    drop(whitelists);

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

    #[test]
    fn test_validate_host() {
        // Valid hosts
        assert!(validate_host("example.com"));
        assert!(validate_host("sub.example.com"));
        assert!(validate_host("example.com:8080"));
        assert!(validate_host("192.168.1.1"));

        // Invalid hosts
        assert!(!validate_host(""));
        assert!(!validate_host("example..com"));
        assert!(!validate_host("example//com"));
        assert!(!validate_host("example\\com"));
        assert!(!validate_host("../etc/passwd"));
    }

    #[test]
    fn test_validate_path() {
        // Valid paths
        assert!(validate_path("file.zip"));
        assert!(validate_path("path/to/file.zip"));
        assert!(validate_path("dist/bundle.js"));

        // Invalid paths
        assert!(!validate_path(""));
        assert!(!validate_path("../etc/passwd"));
        assert!(!validate_path("path/../file"));
        assert!(!validate_path("path//file"));
        assert!(!validate_path("path\\file"));
    }
}
