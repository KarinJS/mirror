use crate::config::AppState;
use crate::error::{AppError, AppResult};
use crate::proxy::{proxy_upstream, ProxyOptions};
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::Response;
use reqwest::Client;

fn validate_username(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 39
        && s.chars().all(|c| c.is_alphanumeric() || c == '-')
        && !s.starts_with('-')
        && !s.ends_with('-')
}

pub async fn handle_avatar(
    State(state): State<AppState>,
    State(client): State<Client>,
    headers: HeaderMap,
    path: String,
) -> AppResult<Response> {
    // Path format: /avatar/<user>.png
    let user = path
        .trim_start_matches("/avatar/")
        .trim_end_matches(".png");

    if !validate_username(user) {
        return Err(AppError::NotFound);
    }

    let whitelists = state.whitelists.read().await;
    let allowed = whitelists.avatar.contains(&user.to_string());

    if !allowed {
        return Err(AppError::NotFound);
    }

    let target = format!("https://github.com/{}.png", user);

    let config = state.config.read().await;
    let ttl = config.cache_ttl.avatar;
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
