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

#[cfg(test)]
mod tests {
    use super::*;

    // ── validate_username ────────────────────────────────────────────────

    #[test]
    fn test_valid_usernames() {
        assert!(validate_username("karinjs"));
        assert!(validate_username("a"));
        assert!(validate_username("NapNeko"));
        assert!(validate_username("user-name"));
        assert!(validate_username("user123"));
        // max length (39)
        assert!(validate_username(&"a".repeat(39)));
    }

    #[test]
    fn test_invalid_usernames() {
        assert!(!validate_username(""));
        assert!(!validate_username("-starts-with-dash"));
        assert!(!validate_username("ends-with-dash-"));
        assert!(!validate_username("has space"));
        assert!(!validate_username("has_underscore"));
        assert!(!validate_username("has/slash"));
        // over 39 chars
        assert!(!validate_username(&"a".repeat(40)));
    }

    // ── path parsing ─────────────────────────────────────────────────────

    #[test]
    fn test_parse_user_from_path() {
        let path = "/avatar/karinjs.png";
        let user = path.trim_start_matches("/avatar/").trim_end_matches(".png");
        assert_eq!(user, "karinjs");
    }

    #[test]
    fn test_parse_user_with_numbers() {
        let path = "/avatar/user123.png";
        let user = path.trim_start_matches("/avatar/").trim_end_matches(".png");
        assert_eq!(user, "user123");
    }

    // ── whitelist logic ───────────────────────────────────────────────────

    #[test]
    fn test_whitelist_membership() {
        let whitelist: Vec<String> = vec!["karinjs".to_string(), "NapNeko".to_string()];
        assert!(whitelist.contains(&"karinjs".to_string()));
        assert!(whitelist.contains(&"NapNeko".to_string()));
        assert!(!whitelist.contains(&"unknown".to_string()));
    }

    // ── target URL construction ───────────────────────────────────────────

    #[test]
    fn test_target_url() {
        let user = "karinjs";
        let target = format!("https://github.com/{}.png", user);
        assert_eq!(target, "https://github.com/karinjs.png");
    }
}

