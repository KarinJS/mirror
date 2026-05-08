use crate::config::{
    AppState, AvatarWhitelist, ConfigSyncUrls, MirrorWhitelist, RawWhitelist,
    ReleasesWhitelist, UnpkgWhitelist,
};
use reqwest::Client;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

// Cap sync response bodies at 10 MB to guard against misconfigured URLs.
const MAX_SYNC_BODY_BYTES: usize = 10 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WhitelistKind {
    Avatar,
    Raw,
    Releases,
    Mirror,
    Unpkg,
}

impl WhitelistKind {
    pub fn all() -> [Self; 5] {
        [Self::Avatar, Self::Raw, Self::Releases, Self::Mirror, Self::Unpkg]
    }

    pub fn filename(self) -> &'static str {
        match self {
            Self::Avatar => "github.avatar.json",
            Self::Raw => "github.raw.json",
            Self::Releases => "github.releases.json",
            Self::Mirror => "mirror.json",
            Self::Unpkg => "unpkg.json",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Avatar => "avatar",
            Self::Raw => "raw",
            Self::Releases => "releases",
            Self::Mirror => "mirror",
            Self::Unpkg => "unpkg",
        }
    }

    fn url(self, urls: &ConfigSyncUrls) -> &str {
        match self {
            Self::Avatar => &urls.avatar,
            Self::Raw => &urls.raw,
            Self::Releases => &urls.releases,
            Self::Mirror => &urls.mirror,
            Self::Unpkg => &urls.unpkg,
        }
    }
}

#[derive(Default)]
struct SyncHashes {
    avatar: Option<String>,
    raw: Option<String>,
    releases: Option<String>,
    mirror: Option<String>,
    unpkg: Option<String>,
}

impl SyncHashes {
    fn get(&self, kind: WhitelistKind) -> &Option<String> {
        match kind {
            WhitelistKind::Avatar => &self.avatar,
            WhitelistKind::Raw => &self.raw,
            WhitelistKind::Releases => &self.releases,
            WhitelistKind::Mirror => &self.mirror,
            WhitelistKind::Unpkg => &self.unpkg,
        }
    }

    fn set(&mut self, kind: WhitelistKind, hash: String) {
        let slot = match kind {
            WhitelistKind::Avatar => &mut self.avatar,
            WhitelistKind::Raw => &mut self.raw,
            WhitelistKind::Releases => &mut self.releases,
            WhitelistKind::Mirror => &mut self.mirror,
            WhitelistKind::Unpkg => &mut self.unpkg,
        };
        *slot = Some(hash);
    }
}

fn hex_sha256(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Validate that a JSON body can be deserialized into the correct whitelist type.
/// Public for testability.
pub fn validate_whitelist_json(kind: WhitelistKind, body: &str) -> Result<(), String> {
    if body.trim().is_empty() {
        return Err("empty body".to_string());
    }
    match kind {
        WhitelistKind::Avatar => {
            let _: AvatarWhitelist =
                serde_json::from_str(body).map_err(|e| format!("invalid avatar json: {e}"))?;
        }
        WhitelistKind::Raw => {
            let _: RawWhitelist =
                serde_json::from_str(body).map_err(|e| format!("invalid raw json: {e}"))?;
        }
        WhitelistKind::Releases => {
            let _: ReleasesWhitelist =
                serde_json::from_str(body).map_err(|e| format!("invalid releases json: {e}"))?;
        }
        WhitelistKind::Mirror => {
            let _: MirrorWhitelist =
                serde_json::from_str(body).map_err(|e| format!("invalid mirror json: {e}"))?;
        }
        WhitelistKind::Unpkg => {
            let _: UnpkgWhitelist =
                serde_json::from_str(body).map_err(|e| format!("invalid unpkg json: {e}"))?;
        }
    }
    Ok(())
}

async fn init_hashes(config_root: &Path) -> SyncHashes {
    let mut hashes = SyncHashes::default();
    for kind in WhitelistKind::all() {
        let path = config_root.join(kind.filename());
        if let Ok(content) = tokio::fs::read_to_string(&path).await {
            let hash = hex_sha256(content.as_bytes());
            hashes.set(kind, hash);
        }
    }
    hashes
}

async fn apply_sync(
    state: &AppState,
    kind: WhitelistKind,
    body: &str,
    config_root: &Path,
    new_hash: &str,
    hashes: &mut SyncHashes,
) -> Result<(), String> {
    validate_whitelist_json(kind, body)?;

    // Write to disk
    let path = config_root.join(kind.filename());
    tokio::fs::write(&path, body)
        .await
        .map_err(|e| format!("write {}: {e}", kind.filename()))?;

    // Hot-reload into memory
    {
        let mut whitelists = state.whitelists.write().await;
        match kind {
            WhitelistKind::Avatar => {
                whitelists.avatar = serde_json::from_str(body).expect("validated above");
            }
            WhitelistKind::Raw => {
                whitelists.raw = serde_json::from_str(body).expect("validated above");
            }
            WhitelistKind::Releases => {
                whitelists.releases = serde_json::from_str(body).expect("validated above");
            }
            WhitelistKind::Mirror => {
                whitelists.mirror = serde_json::from_str(body).expect("validated above");
            }
            WhitelistKind::Unpkg => {
                whitelists.unpkg = serde_json::from_str(body).expect("validated above");
            }
        }
    }

    hashes.set(kind, new_hash.to_string());
    tracing::info!(
        "config sync: {} updated ({} bytes)",
        kind.name(),
        body.len()
    );
    Ok(())
}

/// Fetch, hash, compare, and apply one whitelist file from remote.
///
/// Every error path returns the error as a String rather than panicking,
/// so a single bad URL or malformed response never takes down the task.
async fn sync_one(
    kind: WhitelistKind,
    url: &str,
    config_root: &Path,
    client: &Client,
    state: &AppState,
    hashes: &mut SyncHashes,
) {
    let result: Result<(), String> = async {
        let resp = client
            .get(url)
            .header("User-Agent", "mirror-config-sync/1.0")
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| format!("fetch {}: {e}", url))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(format!("{url} returned HTTP {status}"));
        }

        // Stream body with a hard size cap to avoid OOM on misconfigured URLs.
        let mut buf = Vec::with_capacity(8192);
        let mut stream = resp.bytes_stream();
        use futures::StreamExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("stream read {url}: {e}"))?;
            if buf.len() + chunk.len() > MAX_SYNC_BODY_BYTES {
                return Err(format!(
                    "{url}: body exceeds {} bytes",
                    MAX_SYNC_BODY_BYTES
                ));
            }
            buf.extend_from_slice(&chunk);
        }

        if buf.is_empty() {
            return Err(format!("{url}: empty body"));
        }

        let body = String::from_utf8(buf).map_err(|e| format!("{url}: invalid utf-8: {e}"))?;

        let new_hash = hex_sha256(body.as_bytes());

        if hashes.get(kind).as_deref() == Some(&new_hash) {
            tracing::debug!("config sync: {} unchanged", kind.name());
            return Ok(());
        }

        apply_sync(state, kind, &body, config_root, &new_hash, hashes).await
    }
    .await;

    if let Err(e) = result {
        tracing::warn!("config sync: {} sync failed — {e}", kind.name());
    }
}

pub async fn config_sync_task(state: AppState, client: Client) {
    let config_root = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("config");

    let mut hashes = init_hashes(&config_root).await;

    loop {
        let (enabled, interval_secs, urls) = {
            let cfg = state.config.read().await;
            (
                cfg.config_sync.enabled,
                cfg.config_sync.interval_seconds.max(1),
                cfg.config_sync.urls.clone(),
            )
        };

        if enabled {
            for kind in WhitelistKind::all() {
                let url = kind.url(&urls);
                if url.is_empty() {
                    continue;
                }
                sync_one(kind, url, &config_root, &client, &state, &mut hashes).await;
            }
        } else {
            tracing::debug!("config sync: disabled, sleeping");
        }

        tokio::time::sleep(Duration::from_secs(interval_secs)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── hex_sha256 ────────────────────────────────────────────

    #[test]
    fn test_hex_sha256_known_vector() {
        // Empty string vector from NIST.
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
        // SHA-256 always produces 32 bytes → 64 hex chars.
        for input in &[b"" as &[u8], b"x", b"hello world", &[0u8; 1024]] {
            assert_eq!(hex_sha256(input).len(), 64);
        }
    }

    // ── WhitelistKind ─────────────────────────────────────────

    #[test]
    fn test_all_returns_five() {
        assert_eq!(WhitelistKind::all().len(), 5);
    }

    #[test]
    fn test_filename_mapping() {
        assert_eq!(WhitelistKind::Avatar.filename(), "github.avatar.json");
        assert_eq!(WhitelistKind::Raw.filename(), "github.raw.json");
        assert_eq!(
            WhitelistKind::Releases.filename(),
            "github.releases.json"
        );
        assert_eq!(WhitelistKind::Mirror.filename(), "mirror.json");
        assert_eq!(WhitelistKind::Unpkg.filename(), "unpkg.json");
    }

    #[test]
    fn test_name_mapping() {
        assert_eq!(WhitelistKind::Avatar.name(), "avatar");
        assert_eq!(WhitelistKind::Raw.name(), "raw");
        assert_eq!(WhitelistKind::Releases.name(), "releases");
        assert_eq!(WhitelistKind::Mirror.name(), "mirror");
        assert_eq!(WhitelistKind::Unpkg.name(), "unpkg");
    }

    #[test]
    fn test_all_covers_every_kind() {
        let kinds = WhitelistKind::all();
        let names: Vec<&str> = kinds.iter().map(|k| k.name()).collect();
        assert!(names.contains(&"avatar"));
        assert!(names.contains(&"raw"));
        assert!(names.contains(&"releases"));
        assert!(names.contains(&"mirror"));
        assert!(names.contains(&"unpkg"));
    }

    #[test]
    fn test_debug_format() {
        let s = format!("{:?}", WhitelistKind::Avatar);
        assert_eq!(s, "Avatar");
    }

    // ── validate_whitelist_json ───────────────────────────────

    #[test]
    fn test_validate_avatar_valid() {
        assert!(validate_whitelist_json(WhitelistKind::Avatar, r#"["karinjs"]"#).is_ok());
        assert!(validate_whitelist_json(WhitelistKind::Avatar, "[]").is_ok());
    }

    #[test]
    fn test_validate_avatar_invalid() {
        // Not an array — should fail.
        assert!(validate_whitelist_json(WhitelistKind::Avatar, r#"{"x": 1}"#).is_err());
        // Not valid JSON at all.
        assert!(validate_whitelist_json(WhitelistKind::Avatar, "not json").is_err());
    }

    #[test]
    fn test_validate_raw_valid() {
        let json = r#"{"karinjs":{"karin":[{"branch":"HEAD","file":"package.json"}]}}"#;
        assert!(validate_whitelist_json(WhitelistKind::Raw, json).is_ok());
    }

    #[test]
    fn test_validate_raw_invalid() {
        assert!(validate_whitelist_json(WhitelistKind::Raw, "[]").is_err());
        assert!(validate_whitelist_json(WhitelistKind::Raw, "").is_err());
    }

    #[test]
    fn test_validate_mirror_valid_simple() {
        assert!(
            validate_whitelist_json(WhitelistKind::Mirror, r#"{"https://example.com": 0}"#)
                .is_ok()
        );
    }

    #[test]
    fn test_validate_mirror_valid_complex() {
        let json =
            r#"{"https://example.com":{"ttl":60,"maxSize":1048576}}"#;
        assert!(validate_whitelist_json(WhitelistKind::Mirror, json).is_ok());
    }

    #[test]
    fn test_validate_mirror_invalid() {
        assert!(validate_whitelist_json(WhitelistKind::Mirror, "[]").is_err());
        assert!(validate_whitelist_json(WhitelistKind::Mirror, "not json").is_err());
    }

    #[test]
    fn test_validate_unpkg_valid() {
        let json = r#"{"karin":["package.json"]}"#;
        assert!(validate_whitelist_json(WhitelistKind::Unpkg, json).is_ok());
    }

    #[test]
    fn test_validate_unpkg_invalid() {
        assert!(validate_whitelist_json(WhitelistKind::Unpkg, "42").is_err());
    }

    #[test]
    fn test_validate_releases_valid() {
        let json = r#"{"NapNeko":{"NapCatQQ":["NapCat.Framework.zip"]}}"#;
        assert!(validate_whitelist_json(WhitelistKind::Releases, json).is_ok());
    }

    #[test]
    fn test_validate_releases_invalid() {
        assert!(validate_whitelist_json(WhitelistKind::Releases, "null").is_err());
    }

    #[test]
    fn test_validate_empty_body() {
        assert!(validate_whitelist_json(WhitelistKind::Avatar, "").is_err());
        assert!(validate_whitelist_json(WhitelistKind::Avatar, "   ").is_err());
    }

    #[test]
    fn test_validate_malformed_utf8_equivalent() {
        // Valid JSON but wrong type for every kind.
        let bad = "true";
        assert!(validate_whitelist_json(WhitelistKind::Avatar, bad).is_err());
        assert!(validate_whitelist_json(WhitelistKind::Raw, bad).is_err());
        assert!(validate_whitelist_json(WhitelistKind::Releases, bad).is_err());
        assert!(validate_whitelist_json(WhitelistKind::Mirror, bad).is_err());
        assert!(validate_whitelist_json(WhitelistKind::Unpkg, bad).is_err());
    }

    // ── SyncHashes ────────────────────────────────────────────

    #[test]
    fn test_sync_hashes_default_all_none() {
        let h = SyncHashes::default();
        for kind in WhitelistKind::all() {
            assert!(h.get(kind).is_none());
        }
    }

    #[test]
    fn test_sync_hashes_set_and_get() {
        let mut h = SyncHashes::default();
        h.set(WhitelistKind::Avatar, "abc".into());
        assert_eq!(h.get(WhitelistKind::Avatar).as_deref(), Some("abc"));
        // Other kinds still None.
        assert!(h.get(WhitelistKind::Raw).is_none());
    }
}
