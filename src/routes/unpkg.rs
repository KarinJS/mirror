use crate::config::AppState;
use crate::error::{AppError, AppResult};
use crate::proxy::{resolve_upstream_url, proxy_upstream, ProxyOptions, CacheTtl};
use crate::validation::validate_path_component;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use reqwest::Client;
use std::time::Duration;

const MAX_REDIRECTS: usize = 3;

/// Check if a file path matches a whitelist rule with proper path boundary checking.
fn matches_path_rule(file_path: &str, rule: &str) -> bool {
    if rule.ends_with('/') {
        file_path.starts_with(rule) || file_path == &rule[..rule.len() - 1]
    } else {
        file_path == rule || file_path.ends_with(&format!("/{}", rule))
    }
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

    // Extract package name (strip version if present).
    // Use rfind('@') so scoped packages like @scope/pkg@1.0.0
    // correctly split at the version separator (last @), not the
    // scope prefix (first @).
    let pkg_name = match pkg_part.rfind('@') {
        Some(pos) if pos > 0 => &pkg_part[..pos],
        _ => pkg_part,
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
        .map(|files| files.iter().any(|f| matches_path_rule(file_path, f)))
        .unwrap_or(false);

    if !allowed {
        return Err(AppError::NotFound);
    }

    drop(whitelists);

    let target = format!("https://unpkg.com/{}/{}", pkg_part, file_path);

    // If no version specified, resolve redirect to versioned URL.
    // Compare pkg_part length with pkg_name length: if they differ,
    // a version suffix was present (e.g. pkg@1.0.0 → pkg_name="pkg").
    if pkg_part.len() == pkg_name.len() {
        let config = state.config.read().await;
        let timeout = Duration::from_millis(config.mirror.fetch_timeout_ms);
        drop(config);

        let mut visited = vec![target.clone()];
        let mut current_url = target.clone();

        for _ in 0..MAX_REDIRECTS {
            let resolved = resolve_upstream_url(&client, &headers, &current_url, timeout).await?;

            if resolved == current_url {
                break; // No redirect, use current_url
            }

            // Only follow redirects within unpkg.com
            if !resolved.starts_with("https://unpkg.com/") {
                break;
            }

            // Check for redirect loop
            if visited.contains(&resolved) {
                tracing::warn!("unpkg redirect loop detected: {}", resolved);
                return Err(AppError::BadGateway);
            }

            visited.push(resolved.clone());
            current_url = resolved;
        }

        // If the final resolved URL differs from the original, redirect
        // Only redirect to URLs within unpkg.com (reject cross-domain redirects)
        if current_url != target && current_url.starts_with("https://unpkg.com/") {
            if let Some(unpkg_path) = current_url.strip_prefix("https://unpkg.com/") {
                let mirror_path = format!("/unpkg/{}", unpkg_path);
                return Ok(Redirect::temporary(&mirror_path).into_response());
            }
        }
    }

    let config = state.config.read().await;
    let ttl = CacheTtl::from_config(config.cache_ttl.unpkg);
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
    // ── package name extraction ──

    fn extract_pkg_name(pkg_part: &str) -> &str {
        match pkg_part.rfind('@') {
            Some(pos) if pos > 0 => &pkg_part[..pos],
            _ => pkg_part,
        }
    }

    fn is_valid_pkg_name(name: &str) -> bool {
        !name.is_empty()
            && name.len() <= 214
            && !name.starts_with('.')
            && !name.starts_with('_')
            && name.chars().all(|c| {
                c.is_ascii_lowercase()
                    || c.is_ascii_digit()
                    || c == '-'
                    || c == '_'
                    || c == '/'
                    || c == '@'
            })
    }

    #[test]
    fn test_extract_unversioned() {
        assert_eq!(extract_pkg_name("karin"), "karin");
    }

    #[test]
    fn test_extract_versioned() {
        assert_eq!(extract_pkg_name("karin@0.13.1"), "karin");
    }

    #[test]
    fn test_scoped_package_with_version() {
        assert_eq!(extract_pkg_name("@scope/pkg@1.0.0"), "@scope/pkg");
    }

    #[test]
    fn test_scoped_package_without_version() {
        assert_eq!(extract_pkg_name("@scope/pkg"), "@scope/pkg");
    }

    // ── package name validation ──

    #[test]
    fn test_valid_pkg_names() {
        assert!(is_valid_pkg_name("karin"));
        assert!(is_valid_pkg_name("karin-js"));
        assert!(is_valid_pkg_name("@scope/pkg"));
    }

    #[test]
    fn test_invalid_pkg_names() {
        assert!(!is_valid_pkg_name(""));
        assert!(!is_valid_pkg_name(".hidden"));
        assert!(!is_valid_pkg_name("_internal"));
        assert!(!is_valid_pkg_name("HAS-UPPER"));
    }

    // ── redirect loop detection ──

    #[test]
    fn test_redirect_loop_detection() {
        let visited = [
            "https://unpkg.com/karin/package.json".to_string(),
            "https://unpkg.com/karin@0.13.0/package.json".to_string(),
        ];
        let new = "https://unpkg.com/karin/package.json".to_string();
        assert!(visited.contains(&new));
    }
}
