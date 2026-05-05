use crate::config::AppState;
use crate::error::{AppError, AppResult};
use crate::proxy::{proxy_upstream, ProxyOptions};
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::Response;
use reqwest::Client;

fn validate_path_component(s: &str) -> bool {
    !s.is_empty() && !s.contains("..") && !s.contains("//") && !s.contains('\\')
}

pub async fn handle_releases(
    State(state): State<AppState>,
    State(client): State<Client>,
    headers: HeaderMap,
    path: String,
) -> AppResult<Response> {
    // Path format: /gh/<owner>/<repo>/releases/download/<tag>/<file>
    let parts: Vec<&str> = path.trim_start_matches("/gh/").split('/').collect();
    if parts.len() < 6 || parts[2] != "releases" || parts[3] != "download" {
        return Err(AppError::NotFound);
    }

    let owner = parts[0];
    let repo = parts[1];
    let tag = parts[4];
    let file = parts[5..].join("/");

    // Validate path components
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
        .map(|files| files.iter().any(|f| file.ends_with(f)))
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
        ProxyOptions { ttl: -1, max_size: None }, // immutable
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // ── validate_path_component ──────────────────────────────────────────

    #[test]
    fn test_validate_path_component_valid() {
        assert!(validate_path_component("NapCat.Framework.zip"));
        assert!(validate_path_component("v4.18.0"));
        assert!(validate_path_component("NapNeko"));
        assert!(validate_path_component("subdir/file.zip"));
        assert!(validate_path_component("file-name_v1.0.tar.gz"));
    }

    #[test]
    fn test_validate_path_component_invalid() {
        assert!(!validate_path_component(""));
        assert!(!validate_path_component("../etc/passwd"));
        assert!(!validate_path_component("path/../file"));
        assert!(!validate_path_component("path//file"));
        assert!(!validate_path_component("path\\file"));
    }

    // ── path parsing ─────────────────────────────────────────────────────

    #[test]
    fn test_parse_standard_path() {
        let path = "/gh/NapNeko/NapCatQQ/releases/download/v4.18.0/NapCat.Framework.zip";
        let parts: Vec<&str> = path.trim_start_matches("/gh/").split('/').collect();
        assert_eq!(parts.len(), 6);
        assert_eq!(parts[0], "NapNeko");
        assert_eq!(parts[1], "NapCatQQ");
        assert_eq!(parts[2], "releases");
        assert_eq!(parts[3], "download");
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

    #[test]
    fn test_parse_too_short_is_rejected() {
        let path = "/gh/NapNeko/NapCatQQ";
        let parts: Vec<&str> = path.trim_start_matches("/gh/").split('/').collect();
        assert!(parts.len() < 6);
    }

    #[test]
    fn test_parse_wrong_structure_rejected() {
        // 6 parts but "tags" instead of "releases" at index 2
        let path = "/gh/owner/repo/tags/v1.0/download/file.zip";
        let parts: Vec<&str> = path.trim_start_matches("/gh/").split('/').collect();
        assert_eq!(parts.len(), 6);
        assert_ne!(parts[2], "releases");
    }

    // ── target URL construction ───────────────────────────────────────────

    #[test]
    fn test_target_url_no_gh_prefix() {
        let (owner, repo, tag, file) = ("NapNeko", "NapCatQQ", "v4.18.0", "NapCat.Framework.zip");
        let target = format!(
            "https://github.com/{}/{}/releases/download/{}/{}",
            owner, repo, tag, file
        );
        assert_eq!(
            target,
            "https://github.com/NapNeko/NapCatQQ/releases/download/v4.18.0/NapCat.Framework.zip"
        );
        // Must NOT contain /gh/ in the path
        assert!(!target.contains("/gh/"));
    }

    // ── whitelist logic ───────────────────────────────────────────────────

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

    fn check(wl: &HashMap<String, HashMap<String, Vec<String>>>, owner: &str, repo: &str, file: &str) -> bool {
        wl.get(owner)
            .and_then(|r| r.get(repo))
            .map(|files| files.iter().any(|f| file.ends_with(f.as_str())))
            .unwrap_or(false)
    }

    #[test]
    fn test_whitelist_exact_name_match() {
        let wl = make_whitelist();
        assert!(check(&wl, "NapNeko", "NapCatQQ", "NapCat.Framework.zip"));
    }

    #[test]
    fn test_whitelist_extension_match() {
        let wl = make_whitelist();
        assert!(check(&wl, "NapNeko", "NapCatQQ", "NapCatQQ-v4.18.0.exe"));
        assert!(check(&wl, "NapNeko", "NapCatQQ", "NapCatQQ.AppImage"));
    }

    #[test]
    fn test_whitelist_unknown_owner_rejected() {
        let wl = make_whitelist();
        assert!(!check(&wl, "Unknown", "NapCatQQ", "NapCat.Framework.zip"));
    }

    #[test]
    fn test_whitelist_unknown_repo_rejected() {
        let wl = make_whitelist();
        assert!(!check(&wl, "NapNeko", "OtherRepo", "NapCat.Framework.zip"));
    }

    #[test]
    fn test_whitelist_unmatched_file_rejected() {
        let wl = make_whitelist();
        assert!(!check(&wl, "NapNeko", "NapCatQQ", "SomeOtherFile.tar.gz"));
    }

    // ── path traversal guard ──────────────────────────────────────────────

    #[test]
    fn test_path_traversal_in_owner_rejected() {
        assert!(!validate_path_component("../etc"));
    }

    #[test]
    fn test_path_traversal_in_file_rejected() {
        assert!(!validate_path_component("../../secret.txt"));
    }
}
