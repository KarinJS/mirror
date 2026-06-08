use crate::config::{parse_config, AppState, CONFIG_FILE};
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

/// Validate a freshly-fetched config document and apply ONLY its `whitelists`.
///
/// Security model: the remote sync source is treated as authoritative for the
/// whitelists alone. App settings (`auth`, `host`, `port`, `geo`, `configSync`,
/// …) always come from the local config and are never overwritten by a remote —
/// so even a compromised sync source cannot disable auth, change the listen
/// address, or redirect sync. The full document is still parsed and validated
/// (structure + semantics) so a malformed or malicious payload is rejected
/// before anything is written.
///
/// The merged document (local settings + remote whitelists) is what gets
/// persisted, keeping the on-disk file consistent with this scope across
/// restarts.
async fn apply_sync(state: &AppState, body: &str, path: &Path) -> Result<(), String> {
    let remote = parse_config(body).map_err(|e| format!("invalid config: {e}"))?;

    // Build local-settings + remote-whitelists, validate happened above.
    let mut merged = state.config.read().await.clone();
    merged.whitelists = remote.whitelists;

    let serialized = serde_json::to_string_pretty(&merged)
        .map_err(|e| format!("serialize merged config: {e}"))?;

    tokio::fs::write(path, &serialized)
        .await
        .map_err(|e| format!("write {CONFIG_FILE}: {e}"))?;

    let new_whitelists = merged.whitelists.clone();
    // Update the config snapshot (only whitelists changed) and the dedicated
    // whitelists lock so both stay consistent.
    {
        let mut c = state.config.write().await;
        *c = merged;
    }
    {
        let mut w = state.whitelists.write().await;
        *w = new_whitelists;
    }

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
        .join(CONFIG_FILE);

    // Tracks the hash of the last successfully-applied REMOTE body. The on-disk
    // file is a merged document (local settings + remote whitelists), so it
    // can't be compared against a remote body directly — start fresh and let the
    // first successful fetch apply once.
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

    /// A merged config document with the given `whitelists` block inlined.
    fn config_doc(whitelists: &str) -> String {
        format!(
            r#"{{
            "host": "0.0.0.0", "port": 7878, "publicOrigin": "https://example.com",
            "trustProxyHeaders": true, "logLevel": "info",
            "geo": {{ "mode": "off", "headerName": "h", "countries": [] }},
            "cacheTTL": {{ "raw": 0, "avatar": 0, "unpkg": 0 }},
            "mirror": {{ "defaultTTL": 0, "defaultMaxSize": 1, "absoluteMaxSize": 1, "fetchTimeoutMs": 1 }},
            "cors": {{ "enabledRoutes": [] }},
            "auth": {{ "enabled": false, "key": "", "value": "" }},
            "configSync": {{ "enabled": false, "intervalSeconds": 300, "url": "" }},
            "whitelists": {whitelists}
        }}"#
        )
    }

    /// Build an in-memory AppState from a merged config document.
    fn state_from(doc: &str) -> AppState {
        use std::sync::Arc;
        use tokio::sync::RwLock;
        let config = parse_config(doc).unwrap();
        let whitelists = config.whitelists.clone();
        AppState {
            config: Arc::new(RwLock::new(config)),
            whitelists: Arc::new(RwLock::new(whitelists)),
        }
    }

    #[tokio::test]
    async fn test_apply_sync_valid_updates_state() {
        let dir = std::env::temp_dir().join("mirror-sync-test-valid");
        let _ = tokio::fs::create_dir_all(&dir).await;
        let path = dir.join(CONFIG_FILE);

        // Start empty, then sync a document that adds "karinjs" to the avatar list.
        let state = state_from(&config_doc("{}"));
        assert!(state.whitelists.read().await.avatar.is_empty());

        apply_sync(&state, &config_doc(r#"{ "avatar": ["karinjs"] }"#), &path)
            .await
            .unwrap();

        // disk written and whitelist hot-reloaded into memory
        assert!(tokio::fs::read_to_string(&path).await.is_ok());
        assert_eq!(state.whitelists.read().await.avatar, vec!["karinjs"]);
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn test_apply_sync_invalid_does_not_write() {
        let dir = std::env::temp_dir().join("mirror-sync-test-invalid");
        let _ = tokio::fs::create_dir_all(&dir).await;
        let path = dir.join(CONFIG_FILE);
        let _ = tokio::fs::remove_file(&path).await;

        let state = state_from(&config_doc("{}"));
        let err = apply_sync(&state, "{ not valid json", &path).await.unwrap_err();
        assert!(err.contains("invalid config"));
        // Nothing should have been written to disk.
        assert!(tokio::fs::read_to_string(&path).await.is_err());
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn test_apply_sync_ignores_remote_security_settings() {
        // The remote tries to enable auth, change host/port, and re-point sync.
        // None of that must take effect — only the whitelists are adopted.
        let dir = std::env::temp_dir().join("mirror-sync-test-scope");
        let _ = tokio::fs::create_dir_all(&dir).await;
        let path = dir.join(CONFIG_FILE);

        let state = state_from(&config_doc("{}"));

        let hostile_remote = r#"{
            "host": "10.0.0.9", "port": 1, "publicOrigin": "https://evil.example",
            "trustProxyHeaders": true, "logLevel": "info",
            "geo": { "mode": "deny", "headerName": "h", "countries": ["CN"] },
            "cacheTTL": { "raw": 0, "avatar": 0, "unpkg": 0 },
            "mirror": { "defaultTTL": 0, "defaultMaxSize": 1, "absoluteMaxSize": 1, "fetchTimeoutMs": 1 },
            "cors": { "enabledRoutes": [] },
            "auth": { "enabled": true, "key": "X-Pwn", "value": "secret" },
            "configSync": { "enabled": true, "intervalSeconds": 1, "url": "https://evil.example/c.json" },
            "whitelists": { "avatar": ["karinjs"] }
        }"#;

        apply_sync(&state, hostile_remote, &path).await.unwrap();

        let cfg = state.config.read().await;
        // Whitelists DID update.
        assert_eq!(cfg.whitelists.avatar, vec!["karinjs"]);
        // Security-sensitive settings stayed LOCAL.
        assert_eq!(cfg.host, "0.0.0.0");
        assert_eq!(cfg.port, 7878);
        assert!(!cfg.auth.enabled, "remote must not be able to enable auth");
        assert!(matches!(cfg.geo.mode, crate::config::GeoMode::Off));
        assert!(!cfg.config_sync.enabled, "remote must not re-point sync");
        assert!(cfg.config_sync.url.is_empty());
        drop(cfg);

        // The persisted file is the merged doc and must reflect local settings.
        let on_disk = tokio::fs::read_to_string(&path).await.unwrap();
        let parsed = parse_config(&on_disk).unwrap();
        assert_eq!(parsed.host, "0.0.0.0");
        assert!(!parsed.auth.enabled);
        assert_eq!(parsed.whitelists.avatar, vec!["karinjs"]);
        let _ = tokio::fs::remove_dir_all(&dir).await;
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
