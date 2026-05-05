use crate::config::AppState;
use crate::error::{AppError, AppResult};
use crate::proxy::{resolve_upstream_url, proxy_upstream, ProxyOptions};
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use reqwest::Client;
use std::time::Duration;

fn validate_path_component(s: &str) -> bool {
    !s.is_empty() && !s.contains("..") && !s.contains("//") && !s.contains('\\')
}

pub async fn handle_unpkg(
    State(state): State<AppState>,
    State(client): State<Client>,
    headers: HeaderMap,
    path: String,
) -> AppResult<Response> {
    // Path format: /unpkg/<pkg>[@version]/<file>
    let rest = path.trim_start_matches("/unpkg/");
    let parts: Vec<&str> = rest.splitn(2, '/').collect();
    if parts.is_empty() {
        return Err(AppError::NotFound);
    }

    let pkg_part = parts[0];
    let file_path = if parts.len() > 1 { parts[1] } else { "" };

    // Validate file path
    if !file_path.is_empty() && !validate_path_component(file_path) {
        return Err(AppError::NotFound);
    }

    // Extract package name (strip version if present)
    let pkg_name = if let Some(at_pos) = pkg_part.find('@') {
        if at_pos > 0 {
            &pkg_part[..at_pos]
        } else {
            pkg_part
        }
    } else {
        pkg_part
    };

    // Validate package name (npm naming rules)
    if pkg_name.is_empty()
        || pkg_name.len() > 214
        || pkg_name.starts_with('.')
        || pkg_name.starts_with('_')
        || !pkg_name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_' || c == '/' || c == '@') {
        return Err(AppError::NotFound);
    }

    let whitelists = state.whitelists.read().await;
    let allowed = whitelists
        .unpkg
        .get(pkg_name)
        .map(|files| files.iter().any(|f| file_path.starts_with(f)))
        .unwrap_or(false);

    if !allowed {
        return Err(AppError::NotFound);
    }

    drop(whitelists);

    let target = format!("https://unpkg.com/{}/{}", pkg_part, file_path);

    // If no version specified, resolve redirect to versioned URL
    if !pkg_part.contains('@') {
        let config = state.config.read().await;
        let timeout = Duration::from_millis(config.mirror.fetch_timeout_ms);
        drop(config);

        let resolved = resolve_upstream_url(&client, &headers, &target, timeout).await?;

        if resolved != target {
            // Extract the path after unpkg.com
            if let Some(unpkg_path) = resolved.strip_prefix("https://unpkg.com/") {
                let mirror_path = format!("/unpkg/{}", unpkg_path);
                return Ok(Redirect::permanent(&mirror_path).into_response());
            }
        }
    }

    let config = state.config.read().await;
    let ttl = config.cache_ttl.unpkg;
    drop(config);

    proxy_upstream(
        &state,
        &client,
        &headers,
        &target,
        ProxyOptions { ttl, max_size: None },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_path_component() {
        // Valid paths
        assert!(validate_path_component("package.json"));
        assert!(validate_path_component("dist/index.js"));
        assert!(validate_path_component("src/main.ts"));

        // Invalid paths
        assert!(!validate_path_component(""));
        assert!(!validate_path_component("../etc/passwd"));
        assert!(!validate_path_component("path/../file"));
        assert!(!validate_path_component("path//file"));
        assert!(!validate_path_component("path\\file"));
    }
}
