use crate::config::{AppState, Whitelists, WHITELISTS_FILE};
use crate::validation::validate_sync_url;
use futures::StreamExt;
use reqwest::header::CONTENT_TYPE;
use reqwest::Client;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

// Cap sync response bodies at 10 MB to guard against misconfigured URLs.
const MAX_SYNC_BODY_BYTES: usize = 10 * 1024 * 1024;

fn hex_sha256(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

/// Whether a response `Content-Type` is acceptable for a config document.
///
/// Parameters (e.g. `; charset=utf-8`) are ignored. Accepts JSON types
/// (`application/json`, `text/json`, any `*+json`) **and `text/plain`** — many
/// static hosts, notably `raw.githubusercontent.com`, serve `.json` as
/// `text/plain; charset=utf-8`.
///
/// This is only a coarse early filter. The body is ALWAYS fully parsed and
/// semantically validated as an `AppConfig` afterwards, so a hijacked endpoint
/// that happens to return `text/plain` garbage still gets rejected at parse
/// time. `text/html` (the classic captive-portal / login-page hijack),
/// `application/octet-stream`, and a missing/empty header are rejected here.
fn is_acceptable_content_type(value: &str) -> bool {
    let essence = value
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    matches!(
        essence.as_str(),
        "application/json" | "text/json" | "text/plain"
    ) || essence.ends_with("+json")
}

/// Parse a synced body as a whitelists document (`config.mirror.json`).
///
/// The sync source is the whitelists file (`{avatar, raw, releases, unpkg,
/// mirror}`) only — it never carries app settings, so a public sync source
/// exposes nothing but the whitelists.
pub fn parse_synced_whitelists(body: &str) -> Result<Whitelists, String> {
    serde_json::from_str::<Whitelists>(body).map_err(|e| format!("invalid whitelists: {e}"))
}

/// Validate a freshly-fetched whitelists document and apply it.
///
/// Sync only ever touches the whitelists — the local app settings
/// (`config.json`: auth/host/port/geo/configSync/…) are never read or written
/// here, so a compromised sync source can at most change the whitelists (which
/// remain bound by each route's SSRF / path validation). The body is fully
/// parsed before any write, so malformed payloads are rejected.
async fn apply_sync(state: &AppState, body: &str, path: &Path) -> Result<(), String> {
    let whitelists = parse_synced_whitelists(body)?;

    let serialized = serde_json::to_string_pretty(&whitelists)
        .map_err(|e| format!("serialize whitelists: {e}"))?;
    tokio::fs::write(path, &serialized)
        .await
        .map_err(|e| format!("write {WHITELISTS_FILE}: {e}"))?;

    *state.whitelists.write().await = whitelists;

    tracing::info!("config sync: whitelists updated ({} bytes fetched)", body.len());
    Ok(())
}

/// Fetch the remote config once, compare by hash, and apply it if changed.
///
/// Every error path returns the error as a String rather than panicking, so a
/// single bad URL or malformed response never takes down the task. Note that
/// `host`/`port` changes only take effect on restart — the listener is already
/// bound — while everything else (auth, geo, cacheTTL, whitelists) is live.
async fn sync_once(
    url: &str,
    path: &Path,
    client: &Client,
    state: &AppState,
    last_hash: &mut Option<String>,
) {
    // Validate sync URL (SSRF + DNS rebinding protection)
    if let Err(reason) = validate_sync_url(url).await {
        tracing::warn!("config sync: {reason}");
        return;
    }

    let result: Result<(), String> = async {
        let resp = client
            .get(url)
            .header("User-Agent", "mirror-config-sync/1.0")
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| format!("fetch {url}: {e}"))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(format!("{url} returned HTTP {status}"));
        }

        // Coarse Content-Type filter before reading the body: reject HTML/binary
        // (e.g. a hijacked login page) early. JSON and text/plain pass; the body
        // is still fully parsed and validated as a config below regardless.
        let content_type = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if !is_acceptable_content_type(content_type) {
            return Err(format!(
                "{url}: unacceptable Content-Type for config: {:?}",
                content_type
            ));
        }

        // Stream body with a hard size cap to avoid OOM on misconfigured URLs.
        let mut buf = Vec::with_capacity(8192);
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("stream read {url}: {e}"))?;
            if buf.len() + chunk.len() > MAX_SYNC_BODY_BYTES {
                return Err(format!("{url}: body exceeds {MAX_SYNC_BODY_BYTES} bytes"));
            }
            buf.extend_from_slice(&chunk);
        }

        if buf.is_empty() {
            return Err(format!("{url}: empty body"));
        }

        let body = String::from_utf8(buf).map_err(|e| format!("{url}: invalid utf-8: {e}"))?;

        let new_hash = hex_sha256(body.as_bytes());
        if last_hash.as_deref() == Some(&new_hash) {
            tracing::debug!("config sync: unchanged");
            return Ok(());
        }

        apply_sync(state, &body, path).await?;
        *last_hash = Some(new_hash);
        Ok(())
    }
    .await;

    if let Err(e) = result {
        tracing::warn!("config sync: failed — {e}");
    }
}

pub async fn config_sync_task(state: AppState, client: Client) {
    let path = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("config")
        .join(WHITELISTS_FILE);

    // Tracks the hash of the last successfully-applied REMOTE body. Start fresh
    // so the first successful fetch always applies once.
    let mut last_hash: Option<String> = None;

    loop {
        let (enabled, interval_secs, url) = {
            let cfg = state.config.read().await;
            let raw_interval = cfg.config_sync.interval_seconds;
            if raw_interval == 0 {
                tracing::warn!("config sync: intervalSeconds is 0, using default 300s");
            }
            (
                cfg.config_sync.enabled,
                if raw_interval == 0 { 300 } else { raw_interval.max(1) },
                cfg.config_sync.url.clone(),
            )
        };

        if enabled && !url.is_empty() {
            sync_once(&url, &path, &client, &state, &mut last_hash).await;
        } else if enabled {
            tracing::debug!("config sync: enabled but url is empty, skipping");
        } else {
            tracing::debug!("config sync: disabled, sleeping");
        }

        tokio::time::sleep(Duration::from_secs(interval_secs)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── hex_sha256 ──

    #[test]
    fn test_hex_sha256_known_vector() {
        let hash = hex_sha256(b"");
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_hex_sha256_hello_world() {
        let hash = hex_sha256(b"hello world");
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn test_hex_sha256_deterministic() {
        let a = hex_sha256(b"mirror-karinjs");
        let b = hex_sha256(b"mirror-karinjs");
        assert_eq!(a, b);
    }

    #[test]
    fn test_hex_sha256_different_inputs() {
        let a = hex_sha256(b"foo");
        let b = hex_sha256(b"bar");
        assert_ne!(a, b);
    }

    #[test]
    fn test_hex_sha256_length_is_64() {
        for input in &[b"" as &[u8], b"x", b"hello world", &[0u8; 1024]] {
            assert_eq!(hex_sha256(input).len(), 64);
        }
    }

    // ── apply_sync validation ──

    const APP_CFG: &str = r#"{
        "host": "127.0.0.1", "port": 7878, "publicOrigin": "https://example.com",
        "trustProxyHeaders": true, "logLevel": "info",
        "geo": { "mode": "off", "headerName": "h", "countries": [] },
        "cacheTTL": { "raw": 0, "avatar": 0, "unpkg": 0 },
        "mirror": { "defaultTTL": 0, "defaultMaxSize": 1, "absoluteMaxSize": 1, "fetchTimeoutMs": 1 },
        "cors": { "enabledRoutes": [] },
        "auth": { "enabled": false, "key": "", "value": "" }
    }"#;

    /// Build an in-memory AppState: app settings from APP_CFG + empty whitelists.
    fn test_state() -> AppState {
        use std::sync::Arc;
        use tokio::sync::RwLock;
        let config = crate::config::parse_config(APP_CFG).unwrap();
        AppState {
            config: Arc::new(RwLock::new(config)),
            whitelists: Arc::new(RwLock::new(Whitelists::default())),
        }
    }

    #[tokio::test]
    async fn test_apply_sync_updates_whitelists() {
        let dir = std::env::temp_dir().join("mirror-sync-test-valid");
        let _ = tokio::fs::create_dir_all(&dir).await;
        let path = dir.join(WHITELISTS_FILE);

        let state = test_state();
        assert!(state.whitelists.read().await.avatar.is_empty());

        apply_sync(&state, r#"{ "avatar": ["karinjs"] }"#, &path).await.unwrap();

        assert!(tokio::fs::read_to_string(&path).await.is_ok());
        assert_eq!(state.whitelists.read().await.avatar, vec!["karinjs"]);
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn test_apply_sync_invalid_does_not_write() {
        let dir = std::env::temp_dir().join("mirror-sync-test-invalid");
        let _ = tokio::fs::create_dir_all(&dir).await;
        let path = dir.join(WHITELISTS_FILE);
        let _ = tokio::fs::remove_file(&path).await;

        let state = test_state();
        let err = apply_sync(&state, "{ not valid json", &path).await.unwrap_err();
        assert!(err.contains("whitelists"));
        // Nothing should have been written to disk.
        assert!(tokio::fs::read_to_string(&path).await.is_err());
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn test_apply_sync_does_not_touch_app_config() {
        // Sync writes ONLY the whitelists file; the app config is never touched,
        // and the persisted file is a bare whitelists object (no app settings).
        let dir = std::env::temp_dir().join("mirror-sync-test-scope");
        let _ = tokio::fs::create_dir_all(&dir).await;
        let path = dir.join(WHITELISTS_FILE);

        let state = test_state();
        apply_sync(&state, r#"{ "avatar": ["karinjs"] }"#, &path).await.unwrap();

        let cfg = state.config.read().await;
        assert_eq!(cfg.host, "127.0.0.1");
        assert!(!cfg.auth.enabled);
        drop(cfg);

        let on_disk = tokio::fs::read_to_string(&path).await.unwrap();
        let wl: Whitelists = serde_json::from_str(&on_disk).unwrap();
        assert_eq!(wl.avatar, vec!["karinjs"]);
        assert!(!on_disk.contains("\"host\""), "persisted file must not contain app settings");
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[test]
    fn test_parse_synced_whitelists() {
        let wl = parse_synced_whitelists(r#"{ "avatar": ["a"] }"#).unwrap();
        assert_eq!(wl.avatar, vec!["a"]);
        // garbage rejected
        assert!(parse_synced_whitelists("not json").is_err());
        // a full app-config doc is NOT a whitelists object → rejected
        assert!(parse_synced_whitelists(APP_CFG).is_err());
        // unknown key (typo) rejected
        assert!(parse_synced_whitelists(r#"{ "avatr": ["x"] }"#).is_err());
    }

    // ── is_acceptable_content_type ──

    #[test]
    fn test_acceptable_content_type_accepts() {
        assert!(is_acceptable_content_type("application/json"));
        assert!(is_acceptable_content_type("application/json; charset=utf-8"));
        assert!(is_acceptable_content_type("Application/JSON"));
        assert!(is_acceptable_content_type("  application/json  "));
        assert!(is_acceptable_content_type("text/json"));
        assert!(is_acceptable_content_type("application/vnd.api+json"));
        // GitHub raw and many static hosts serve .json as text/plain.
        assert!(is_acceptable_content_type("text/plain"));
        assert!(is_acceptable_content_type("text/plain; charset=utf-8"));
    }

    #[test]
    fn test_acceptable_content_type_rejects() {
        // Empty/HTML/binary are rejected early (the body parse is the real guard).
        assert!(!is_acceptable_content_type(""));
        assert!(!is_acceptable_content_type("text/html"));
        assert!(!is_acceptable_content_type("text/html; charset=utf-8"));
        assert!(!is_acceptable_content_type("application/octet-stream"));
        assert!(!is_acceptable_content_type("application/jsonish"));
    }
}
