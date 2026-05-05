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

    let target = format!("https://github.com{}", path);

    proxy_upstream(
        &state,
        &client,
        &headers,
        &target,
        ProxyOptions { ttl: -1, max_size: None }, // immutable
    )
    .await
}
