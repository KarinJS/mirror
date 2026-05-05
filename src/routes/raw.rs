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
