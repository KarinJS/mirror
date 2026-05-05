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

pub async fn handle_raw(
    State(state): State<AppState>,
    State(client): State<Client>,
    headers: HeaderMap,
    path: String,
) -> AppResult<Response> {
    // Path format: /raw/<owner>/<repo>/<branch>/<path>
    let parts: Vec<&str> = path.trim_start_matches("/raw/").splitn(4, '/').collect();
    if parts.len() < 4 {
        return Err(AppError::NotFound);
    }

    let owner = parts[0];
    let repo = parts[1];
    let branch = parts[2];
    let file_path = parts[3];

    // Validate path components
    if !validate_path_component(owner)
        || !validate_path_component(repo)
        || !validate_path_component(branch)
        || !validate_path_component(file_path) {
        return Err(AppError::NotFound);
    }

    let whitelists = state.whitelists.read().await;
    let allowed = whitelists
        .raw
        .get(owner)
        .and_then(|repos| repos.get(repo))
        .map(|rules| {
            rules.iter().any(|rule| {
                rule.branch == branch && file_path.starts_with(&rule.file)
            })
        })
        .unwrap_or(false);

    if !allowed {
        return Err(AppError::NotFound);
    }

    let target = format!(
        "https://raw.githubusercontent.com/{}/{}/{}/{}",
        owner, repo, branch, file_path
    );

    let config = state.config.read().await;
    let ttl = config.cache_ttl.raw;
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
    use crate::config::RawFileRule;
    use std::collections::HashMap;

    // ── validate_path_component ──────────────────────────────────────────

    #[test]
    fn test_valid_components() {
        assert!(validate_path_component("file.json"));
        assert!(validate_path_component("HEAD"));
        assert!(validate_path_component("main"));
        assert!(validate_path_component("feature/my-branch")); // branch with slash allowed at path level
        assert!(validate_path_component("src/index.ts"));
    }

    #[test]
    fn test_invalid_components() {
        assert!(!validate_path_component(""));
        assert!(!validate_path_component("path/../secret"));
        assert!(!validate_path_component("path//file"));
        assert!(!validate_path_component("path\\file"));
        assert!(!validate_path_component(".."));
    }

    // ── path parsing ─────────────────────────────────────────────────────

    #[test]
    fn test_parse_simple_path() {
        let path = "/raw/karinjs/karin/HEAD/package.json";
        let parts: Vec<&str> = path.trim_start_matches("/raw/").splitn(4, '/').collect();
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[0], "karinjs");
        assert_eq!(parts[1], "karin");
        assert_eq!(parts[2], "HEAD");
        assert_eq!(parts[3], "package.json");
    }

    #[test]
    fn test_parse_nested_file_path() {
        let path = "/raw/karinjs/karin/main/src/utils/index.ts";
        let parts: Vec<&str> = path.trim_start_matches("/raw/").splitn(4, '/').collect();
        assert_eq!(parts.len(), 4);
        // splitn(4) preserves the rest as one part
        assert_eq!(parts[3], "src/utils/index.ts");
    }

    #[test]
    fn test_parse_too_short_rejected() {
        let path = "/raw/karinjs/karin";
        let parts: Vec<&str> = path.trim_start_matches("/raw/").splitn(4, '/').collect();
        assert!(parts.len() < 4);
    }

    // ── whitelist logic ───────────────────────────────────────────────────

    fn make_whitelist() -> HashMap<String, HashMap<String, Vec<RawFileRule>>> {
        let mut inner = HashMap::new();
        inner.insert(
            "karin".to_string(),
            vec![
                RawFileRule { branch: "HEAD".to_string(), file: "package.json".to_string() },
                RawFileRule { branch: "main".to_string(), file: "src/".to_string() },
            ],
        );
        let mut wl = HashMap::new();
        wl.insert("karinjs".to_string(), inner);
        wl
    }

    fn check(
        wl: &HashMap<String, HashMap<String, Vec<RawFileRule>>>,
        owner: &str,
        repo: &str,
        branch: &str,
        file_path: &str,
    ) -> bool {
        wl.get(owner)
            .and_then(|r| r.get(repo))
            .map(|rules| {
                rules
                    .iter()
                    .any(|rule| rule.branch == branch && file_path.starts_with(&rule.file))
            })
            .unwrap_or(false)
    }

    #[test]
    fn test_exact_file_match() {
        let wl = make_whitelist();
        assert!(check(&wl, "karinjs", "karin", "HEAD", "package.json"));
    }

    #[test]
    fn test_prefix_match_under_allowed_dir() {
        let wl = make_whitelist();
        assert!(check(&wl, "karinjs", "karin", "main", "src/index.ts"));
        assert!(check(&wl, "karinjs", "karin", "main", "src/utils/helper.ts"));
    }

    #[test]
    fn test_wrong_branch_rejected() {
        let wl = make_whitelist();
        // "package.json" is only allowed on HEAD
        assert!(!check(&wl, "karinjs", "karin", "dev", "package.json"));
    }

    #[test]
    fn test_unknown_repo_rejected() {
        let wl = make_whitelist();
        assert!(!check(&wl, "karinjs", "other-repo", "HEAD", "package.json"));
    }

    #[test]
    fn test_unknown_owner_rejected() {
        let wl = make_whitelist();
        assert!(!check(&wl, "unknown-org", "karin", "HEAD", "package.json"));
    }

    #[test]
    fn test_file_outside_allowed_dir_rejected() {
        let wl = make_whitelist();
        // "dist/" is not in the whitelist
        assert!(!check(&wl, "karinjs", "karin", "main", "dist/bundle.js"));
    }

    // ── target URL construction ───────────────────────────────────────────

    #[test]
    fn test_target_url() {
        let target = format!(
            "https://raw.githubusercontent.com/{}/{}/{}/{}",
            "karinjs", "karin", "HEAD", "package.json"
        );
        assert_eq!(
            target,
            "https://raw.githubusercontent.com/karinjs/karin/HEAD/package.json"
        );
    }
}
