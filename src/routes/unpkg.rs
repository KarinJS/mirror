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
    use std::collections::HashMap;

    // duplicate the validation logic for testing without needing to call the handler
    fn extract_pkg_name(pkg_part: &str) -> &str {
        if let Some(at_pos) = pkg_part.find('@') {
            if at_pos > 0 { &pkg_part[..at_pos] } else { pkg_part }
        } else {
            pkg_part
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

    // ── validate_path_component ──────────────────────────────────────────

    #[test]
    fn test_valid_file_paths() {
        assert!(validate_path_component("package.json"));
        assert!(validate_path_component("dist/karin.umd.js"));
        assert!(validate_path_component("lib/index.cjs"));
    }

    #[test]
    fn test_invalid_file_paths() {
        assert!(!validate_path_component(""));
        assert!(!validate_path_component("../secret"));
        assert!(!validate_path_component("dist//file.js"));
        assert!(!validate_path_component("dist\\file.js"));
    }

    // ── package name extraction ───────────────────────────────────────────

    #[test]
    fn test_extract_unversioned() {
        assert_eq!(extract_pkg_name("karin"), "karin");
    }

    #[test]
    fn test_extract_versioned() {
        assert_eq!(extract_pkg_name("karin@0.13.1"), "karin");
        assert_eq!(extract_pkg_name("lodash@4.17.21"), "lodash");
    }

    #[test]
    fn test_scoped_package_at_zero_kept_whole() {
        // @ at position 0 means it's a scoped package prefix; we keep the full string
        let pkg_part = "@scope/pkg@1.0.0";
        let name = extract_pkg_name(pkg_part);
        // at_pos == 0, so we return pkg_part itself
        assert_eq!(name, "@scope/pkg@1.0.0");
    }

    // ── package name validation ───────────────────────────────────────────

    #[test]
    fn test_valid_pkg_names() {
        assert!(is_valid_pkg_name("karin"));
        assert!(is_valid_pkg_name("karin-js"));
        assert!(is_valid_pkg_name("karin_utils"));
        assert!(is_valid_pkg_name("pkg123"));
    }

    #[test]
    fn test_invalid_pkg_names() {
        assert!(!is_valid_pkg_name(""));
        assert!(!is_valid_pkg_name(".hidden"));
        assert!(!is_valid_pkg_name("_internal"));
        assert!(!is_valid_pkg_name("HAS-UPPER"));
        assert!(!is_valid_pkg_name("has space"));
        let too_long = "a".repeat(215);
        assert!(!is_valid_pkg_name(&too_long));
    }

    // ── path parsing ─────────────────────────────────────────────────────

    #[test]
    fn test_parse_with_version_and_file() {
        let path = "/unpkg/karin@0.13.1/package.json";
        let rest = path.trim_start_matches("/unpkg/");
        let parts: Vec<&str> = rest.splitn(2, '/').collect();
        assert_eq!(parts[0], "karin@0.13.1");
        assert_eq!(parts[1], "package.json");
        assert!(parts[0].contains('@'));
    }

    #[test]
    fn test_parse_without_version() {
        let path = "/unpkg/karin/package.json";
        let rest = path.trim_start_matches("/unpkg/");
        let parts: Vec<&str> = rest.splitn(2, '/').collect();
        assert_eq!(parts[0], "karin");
        assert_eq!(parts[1], "package.json");
        assert!(!parts[0].contains('@'));
    }

    #[test]
    fn test_parse_no_file() {
        let path = "/unpkg/karin";
        let rest = path.trim_start_matches("/unpkg/");
        let parts: Vec<&str> = rest.splitn(2, '/').collect();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0], "karin");
    }

    // ── whitelist logic ───────────────────────────────────────────────────

    fn make_whitelist() -> HashMap<String, Vec<String>> {
        let mut wl = HashMap::new();
        wl.insert(
            "karin".to_string(),
            vec!["package.json".to_string(), "dist/".to_string()],
        );
        wl
    }

    fn check(wl: &HashMap<String, Vec<String>>, pkg: &str, file: &str) -> bool {
        wl.get(pkg)
            .map(|files| files.iter().any(|f| file.starts_with(f.as_str())))
            .unwrap_or(false)
    }

    #[test]
    fn test_exact_file_allowed() {
        let wl = make_whitelist();
        assert!(check(&wl, "karin", "package.json"));
    }

    #[test]
    fn test_prefix_dir_allowed() {
        let wl = make_whitelist();
        assert!(check(&wl, "karin", "dist/karin.umd.js"));
        assert!(check(&wl, "karin", "dist/karin.cjs.js"));
    }

    #[test]
    fn test_unallowed_file_rejected() {
        let wl = make_whitelist();
        assert!(!check(&wl, "karin", "src/index.ts"));
        assert!(!check(&wl, "karin", "README.md"));
    }

    #[test]
    fn test_unknown_package_rejected() {
        let wl = make_whitelist();
        assert!(!check(&wl, "other-pkg", "package.json"));
    }

    // ── target URL construction ───────────────────────────────────────────

    #[test]
    fn test_target_url_with_version() {
        let target = format!("https://unpkg.com/{}/{}", "karin@0.13.1", "package.json");
        assert_eq!(target, "https://unpkg.com/karin@0.13.1/package.json");
    }

    #[test]
    fn test_target_url_without_version() {
        let target = format!("https://unpkg.com/{}/{}", "karin", "package.json");
        assert_eq!(target, "https://unpkg.com/karin/package.json");
    }

    // ── mirror path extraction from resolved URL ──────────────────────────

    #[test]
    fn test_mirror_path_from_resolved_url() {
        let resolved = "https://unpkg.com/karin@0.13.1/package.json";
        let mirror_path = resolved
            .strip_prefix("https://unpkg.com/")
            .map(|p| format!("/unpkg/{}", p));
        assert_eq!(mirror_path, Some("/unpkg/karin@0.13.1/package.json".to_string()));
    }

    #[test]
    fn test_mirror_path_non_unpkg_url_not_rewritten() {
        // If somehow resolve returns a non-unpkg.com URL, we should NOT rewrite it
        let resolved = "https://cdn.jsdelivr.net/npm/karin@0.13.1/package.json";
        let mirror_path = resolved
            .strip_prefix("https://unpkg.com/")
            .map(|p| format!("/unpkg/{}", p));
        assert!(mirror_path.is_none());
    }
}


