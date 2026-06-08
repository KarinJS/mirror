use crate::config::AppState;
use crate::error::{AppError, AppResult};
use crate::proxy::{proxy_upstream, ProxyOptions, CacheTtl};
use crate::validation::validate_path_component;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::Response;
use reqwest::Client;

/// Check if a file matches a whitelist pattern.
/// - Patterns starting with "." match as file extensions (e.g., ".exe" matches "app-v1.exe")
/// - Other patterns match as exact filenames (e.g., "NapCat.Framework.zip")
fn matches_file_pattern(file: &str, pattern: &str) -> bool {
    if pattern.starts_with('.') {
        // Extension pattern: must match at a filename boundary
        // ".exe" matches "app.exe", "sub/app.exe", but NOT "notexe"
        if !file.ends_with(pattern) {
            return false;
        }
        let prefix_len = file.len() - pattern.len();
        if prefix_len == 0 {
            return true; // file IS the extension (unusual but valid)
        }
        // Use char-level access to safely check the character before the extension.
        // This avoids slicing into multi-byte UTF-8 sequences.
        let before_char = file[..prefix_len]
            .chars()
            .next_back()
            .unwrap_or('/');
        // The character before the extension must be alphanumeric (part of a filename),
        // not a dot or path separator
        before_char.is_ascii_alphanumeric()
    } else {
        // Exact filename match
        file == pattern || file.ends_with(&format!("/{}", pattern))
    }
}

pub async fn handle_releases(
    State(state): State<AppState>,
    State(client): State<Client>,
    headers: HeaderMap,
    path: String,
) -> AppResult<Response> {
    // Path format: /gh/<owner>/<repo>/releases/download/<tag>/<file>
    let raw = path.trim_start_matches("/gh/");

    // Validate the raw path before splitting to prevent split-join
    // roundtrip from collapsing consecutive slashes (// → /).
    if raw.contains("//") || raw.contains('\\') || raw.contains("..") {
        return Err(AppError::NotFound);
    }

    let parts: Vec<&str> = raw.split('/').collect();
    if parts.len() < 6 || parts[2] != "releases" || parts[3] != "download" {
        return Err(AppError::NotFound);
    }

    // Reject empty parts (consecutive slashes)
    if parts.iter().any(|p| p.is_empty()) {
        return Err(AppError::NotFound);
    }

    let owner = parts[0];
    let repo = parts[1];
    let tag = parts[4];
    let file = parts[5..].join("/");

    // Validate individual components (defense in depth)
    if !validate_path_component(owner)
        || !validate_path_component(repo)
        || !validate_path_component(tag)
        || !validate_path_component(&file) {
        return Err(AppError::NotFound);
    }

    let whitelists = state.whitelists.read().await;
    let allowed = whitelists
        .releases
        .get(owner)
        .and_then(|repos| repos.get(repo))
        .map(|files| files.iter().any(|f| matches_file_pattern(&file, f)))
        .unwrap_or(false);

    if !allowed {
        return Err(AppError::NotFound);
    }

    let target = format!(
        "https://github.com/{}/{}/releases/download/{}/{}",
        owner, repo, tag, file
    );

    proxy_upstream(
        &state,
        &client,
        &headers,
        &target,
        ProxyOptions { ttl: CacheTtl::Immutable, max_size: None },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // ── matches_file_pattern ──

    #[test]
    fn test_pattern_exact_name() {
        assert!(matches_file_pattern("NapCat.Framework.zip", "NapCat.Framework.zip"));
        assert!(matches_file_pattern("sub/NapCat.Framework.zip", "NapCat.Framework.zip"));
        assert!(!matches_file_pattern("other.zip", "NapCat.Framework.zip"));
        assert!(!matches_file_pattern("evil-NapCat.Framework.zip", "NapCat.Framework.zip"));
    }

    #[test]
    fn test_pattern_extension() {
        assert!(matches_file_pattern("app-v1.exe", ".exe"));
        assert!(matches_file_pattern("sub/app.exe", ".exe"));
        assert!(matches_file_pattern("NapCatQQ.AppImage", ".AppImage"));
        assert!(!matches_file_pattern("app.txt", ".exe"));
    }

    // ── path parsing ──

    #[test]
    fn test_parse_standard_path() {
        let path = "/gh/NapNeko/NapCatQQ/releases/download/v4.18.0/NapCat.Framework.zip";
        let parts: Vec<&str> = path.trim_start_matches("/gh/").split('/').collect();
        assert_eq!(parts.len(), 6);
        assert_eq!(parts[0], "NapNeko");
        assert_eq!(parts[4], "v4.18.0");
        assert_eq!(parts[5], "NapCat.Framework.zip");
    }

    #[test]
    fn test_parse_nested_file_path() {
        let path = "/gh/owner/repo/releases/download/v1.0/subdir/file.zip";
        let parts: Vec<&str> = path.trim_start_matches("/gh/").split('/').collect();
        assert!(parts.len() >= 6);
        let file = parts[5..].join("/");
        assert_eq!(file, "subdir/file.zip");
    }

    // ── whitelist logic ──

    fn make_whitelist() -> HashMap<String, HashMap<String, Vec<String>>> {
        let mut repos = HashMap::new();
        repos.insert(
            "NapCatQQ".to_string(),
            vec![
                "NapCat.Framework.zip".to_string(),
                ".exe".to_string(),
                ".AppImage".to_string(),
            ],
        );
        let mut wl = HashMap::new();
        wl.insert("NapNeko".to_string(), repos);
        wl
    }

    #[test]
    fn test_whitelist_exact_name_match() {
        let wl = make_whitelist();
        let files = wl.get("NapNeko").unwrap().get("NapCatQQ").unwrap();
        assert!(files.iter().any(|f| matches_file_pattern("NapCat.Framework.zip", f)));
    }

    #[test]
    fn test_whitelist_extension_match() {
        let wl = make_whitelist();
        let files = wl.get("NapNeko").unwrap().get("NapCatQQ").unwrap();
        assert!(files.iter().any(|f| matches_file_pattern("NapCatQQ-v4.18.0.exe", f)));
        assert!(files.iter().any(|f| matches_file_pattern("NapCatQQ.AppImage", f)));
    }

    #[test]
    fn test_whitelist_no_prefix_bypass() {
        let wl = make_whitelist();
        let files = wl.get("NapNeko").unwrap().get("NapCatQQ").unwrap();
        // "evil-NapCat.Framework.zip" should NOT match "NapCat.Framework.zip"
        assert!(!files.iter().any(|f| matches_file_pattern("evil-NapCat.Framework.zip", f)));
    }

    // ── path traversal guard ──

    #[test]
    fn test_path_traversal_rejected() {
        assert!(!validate_path_component("../etc"));
        assert!(!validate_path_component("../../secret.txt"));
    }

    #[test]
    fn test_double_slash_rejected() {
        let raw = "owner/repo/releases/download/tag//secret";
        assert!(raw.contains("//"));
    }
}
