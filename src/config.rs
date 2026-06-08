use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::{Context, Result};
use tracing::debug;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GeoConfig {
    pub mode: GeoMode,
    #[serde(rename = "headerName")]
    pub header_name: String,
    pub countries: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GeoMode {
    Off,
    Allow,
    Deny,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MirrorRuntimeConfig {
    #[serde(rename = "defaultTTL")]
    pub default_ttl: i32,
    #[serde(rename = "defaultMaxSize")]
    pub default_max_size: usize,
    #[serde(rename = "absoluteMaxSize")]
    pub absolute_max_size: usize,
    #[serde(rename = "fetchTimeoutMs")]
    pub fetch_timeout_ms: u64,
    /// Max number of retries when fetching from upstream fails with a
    /// retryable error (connect/timeout/5xx). Total attempts = retries + 1.
    #[serde(rename = "fetchRetries", default = "default_fetch_retries")]
    pub fetch_retries: u32,
}

fn default_fetch_retries() -> u32 { 2 }

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RouteCacheTTLConfig {
    pub raw: i32,
    pub avatar: i32,
    pub unpkg: i32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CorsConfig {
    #[serde(rename = "enabledRoutes")]
    pub enabled_routes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthConfig {
    pub enabled: bool,
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigSyncConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(rename = "intervalSeconds", default = "default_sync_interval")]
    pub interval_seconds: u64,
    /// Direct URL to a remote `config.mirror.json`. When sync is enabled the
    /// whole merged config (app settings + whitelists) is pulled from here.
    #[serde(default)]
    pub url: String,
}

fn default_sync_interval() -> u64 { 300 }

impl Default for ConfigSyncConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_seconds: default_sync_interval(),
            url: String::new(),
        }
    }
}

/// Tencent EdgeOne "origin protection" (源站保护) auto-pull.
///
/// When enabled, the app periodically calls the EO `DescribeOriginACL` API for
/// the configured `zoneId` and writes EO's back-to-origin IP ranges into an
/// nftables ruleset, so only EO 回源 can reach the guarded ports. Requires the
/// app to run as root on Linux with `nft` available. Credentials are sensitive;
/// the merged config file is gitignored.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OriginProtectionConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(rename = "zoneId", default)]
    pub zone_id: String,
    #[serde(rename = "secretId", default)]
    pub secret_id: String,
    #[serde(rename = "secretKey", default)]
    pub secret_key: String,
    /// How often to poll the EO API. EO recommends ~3 days.
    #[serde(rename = "intervalSeconds", default = "default_origin_acl_interval")]
    pub interval_seconds: u64,
    /// Ports to guard (only EO 回源 IPs may reach these). Other ports (e.g. SSH)
    /// are untouched.
    #[serde(default = "default_origin_acl_ports")]
    pub ports: Vec<u16>,
}

fn default_origin_acl_interval() -> u64 { 259_200 } // 3 days
fn default_origin_acl_ports() -> Vec<u16> { vec![80, 443] }

impl Default for OriginProtectionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            zone_id: String::new(),
            secret_id: String::new(),
            secret_key: String::new(),
            interval_seconds: default_origin_acl_interval(),
            ports: default_origin_acl_ports(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    pub host: String,
    pub port: u16,
    #[serde(rename = "publicOrigin")]
    pub public_origin: String,
    #[serde(rename = "trustProxyHeaders")]
    pub trust_proxy_headers: bool,
    #[serde(rename = "logLevel")]
    pub log_level: String,
    pub geo: GeoConfig,
    #[serde(rename = "cacheTTL")]
    pub cache_ttl: RouteCacheTTLConfig,
    pub mirror: MirrorRuntimeConfig,
    pub cors: CorsConfig,
    pub auth: AuthConfig,
    #[serde(rename = "configSync", default)]
    pub config_sync: ConfigSyncConfig,
    #[serde(rename = "originProtection", default)]
    pub origin_protection: OriginProtectionConfig,
    #[serde(default)]
    pub whitelists: Whitelists,
}

pub type AvatarWhitelist = Vec<String>;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RawFileRule {
    pub branch: String,
    pub file: String,
}

pub type RawWhitelist = HashMap<String, HashMap<String, Vec<RawFileRule>>>;
pub type ReleasesWhitelist = HashMap<String, HashMap<String, Vec<String>>>;
pub type UnpkgWhitelist = HashMap<String, Vec<String>>;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum MirrorRule {
    Simple(i32),
    Complex {
        ttl: i32,
        #[serde(rename = "maxSize", skip_serializing_if = "Option::is_none")]
        max_size: Option<usize>,
    },
}

pub type MirrorWhitelist = HashMap<String, MirrorRule>;

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Whitelists {
    #[serde(default)]
    pub avatar: AvatarWhitelist,
    #[serde(default)]
    pub raw: RawWhitelist,
    #[serde(default)]
    pub releases: ReleasesWhitelist,
    #[serde(default)]
    pub unpkg: UnpkgWhitelist,
    #[serde(default)]
    pub mirror: MirrorWhitelist,
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<AppConfig>>,
    pub whitelists: Arc<RwLock<Whitelists>>,
}

/// Name of the single merged config file holding both app settings and all
/// whitelists.
pub const CONFIG_FILE: &str = "config.mirror.json";

async fn generate_defaults(config_root: &Path) -> Result<()> {
    debug!("config not found, generating defaults");
    tokio::fs::create_dir_all(config_root).await?;

    let config = serde_json::json!({
        "host": "0.0.0.0",
        "port": 7878,
        "publicOrigin": "https://mirror.karinjs.com",
        "trustProxyHeaders": true,
        "logLevel": "info",
        "geo": {
            "mode": "off",
            "headerName": "EO-Client-IPCountry",
            "countries": ["CN"]
        },
        "cacheTTL": { "raw": 300, "avatar": 86400, "unpkg": 300 },
        "mirror": {
            "defaultTTL": 0,
            "defaultMaxSize": 52428800,
            "absoluteMaxSize": 1073741824,
            "fetchTimeoutMs": 30000,
            "fetchRetries": 2
        },
        "cors": { "enabledRoutes": ["raw", "unpkg", "mirror"] },
        "auth": { "enabled": false, "key": "", "value": "" },
        "configSync": {
            "enabled": false,
            "intervalSeconds": 300,
            "url": ""
        },
        "originProtection": {
            "enabled": false,
            "zoneId": "",
            "secretId": "",
            "secretKey": "",
            "intervalSeconds": 259200,
            "ports": [80, 443]
        },
        "whitelists": {
            "avatar": ["karinjs"],
            "raw": {
                "karinjs": {
                    "karin": [{"branch": "HEAD", "file": "package.json"}]
                }
            },
            "releases": {
                "NapNeko": {
                    "NapCatQQ": ["NapCat.Framework.zip"]
                }
            },
            "unpkg": {
                "karin": ["package.json", "dist/karin.umd.js"]
            },
            "mirror": {
                "https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions.json": 0
            }
        }
    });

    let text = serde_json::to_string_pretty(&config)?;
    tokio::fs::write(config_root.join(CONFIG_FILE), text).await?;
    debug!("default config generated");

    Ok(())
}

/// Parse a merged config document and run semantic validation on it.
///
/// Shared by the initial load and the config-sync hot-reload path so both
/// reject invalid documents identically.
pub fn parse_config(text: &str) -> Result<AppConfig> {
    let config: AppConfig = serde_json::from_str(text)?;
    validate_config(&config)?;
    Ok(config)
}

/// Validate semantic constraints on a loaded `AppConfig`.
///
/// Pure (no IO) so it can be unit-tested directly. Kept behaviourally identical
/// to the checks that previously lived inline in `AppState::load`.
fn validate_config(config: &AppConfig) -> Result<()> {
    if config.mirror.default_max_size > config.mirror.absolute_max_size {
        anyhow::bail!(
            "mirror.defaultMaxSize ({}) must be <= mirror.absoluteMaxSize ({})",
            config.mirror.default_max_size,
            config.mirror.absolute_max_size
        );
    }
    for (name, ttl) in [
        ("cacheTTL.raw", config.cache_ttl.raw),
        ("cacheTTL.avatar", config.cache_ttl.avatar),
        ("cacheTTL.unpkg", config.cache_ttl.unpkg),
    ] {
        // -2, -1, 0 are valid sentinel values; other negatives are invalid
        if ttl < -2 {
            anyhow::bail!("{name} value {ttl} is invalid (must be >= -2)");
        }
    }
    Ok(())
}

impl AppState {
    pub async fn load() -> Result<Self> {
        let config_root = std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join("config");
        let config_path = config_root.join(CONFIG_FILE);

        if !config_path.is_file() {
            generate_defaults(&config_root).await?;
        }

        let text = tokio::fs::read_to_string(&config_path)
            .await
            .with_context(|| format!("reading {CONFIG_FILE}"))?;
        let config = parse_config(&text).with_context(|| format!("parsing {CONFIG_FILE}"))?;

        // Whitelists live in their own lock so request handlers read them without
        // contending on the full config; seed it from the merged document.
        let whitelists = config.whitelists.clone();

        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            whitelists: Arc::new(RwLock::new(whitelists)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A complete config JSON with a placeholder for the `mirror` block so each
    /// test can substitute it. All other required fields are present.
    fn config_json_with_mirror(mirror_block: &str) -> String {
        format!(
            r#"{{
            "host": "0.0.0.0",
            "port": 7878,
            "publicOrigin": "https://example.com",
            "trustProxyHeaders": true,
            "logLevel": "info",
            "geo": {{ "mode": "off", "headerName": "EO-Client-IPCountry", "countries": ["CN"] }},
            "cacheTTL": {{ "raw": 300, "avatar": 86400, "unpkg": 300 }},
            "mirror": {mirror_block},
            "cors": {{ "enabledRoutes": ["raw", "unpkg", "mirror"] }},
            "auth": {{ "enabled": false, "key": "", "value": "" }}
        }}"#
        )
    }

    // ── fetchRetries backward compatibility ──────────────────────────────

    #[test]
    fn test_old_config_without_fetch_retries_defaults_to_2() {
        // An older config.json that predates fetchRetries must still deserialize,
        // defaulting fetch_retries to 2.
        let mirror = r#"{
            "defaultTTL": 0,
            "defaultMaxSize": 52428800,
            "absoluteMaxSize": 1073741824,
            "fetchTimeoutMs": 30000
        }"#;
        let cfg: AppConfig = serde_json::from_str(&config_json_with_mirror(mirror)).unwrap();
        assert_eq!(cfg.mirror.fetch_retries, 2);
    }

    #[test]
    fn test_new_config_reads_fetch_retries() {
        let mirror = r#"{
            "defaultTTL": 0,
            "defaultMaxSize": 52428800,
            "absoluteMaxSize": 1073741824,
            "fetchTimeoutMs": 30000,
            "fetchRetries": 5
        }"#;
        let cfg: AppConfig = serde_json::from_str(&config_json_with_mirror(mirror)).unwrap();
        assert_eq!(cfg.mirror.fetch_retries, 5);
    }

    #[test]
    fn test_default_fetch_retries_fn() {
        assert_eq!(default_fetch_retries(), 2);
    }

    // ── config_sync defaults ─────────────────────────────────────────────

    #[test]
    fn test_config_sync_defaults_when_absent() {
        // configSync is omitted entirely (uses #[serde(default)]).
        let mirror = r#"{
            "defaultTTL": 0,
            "defaultMaxSize": 1,
            "absoluteMaxSize": 1,
            "fetchTimeoutMs": 30000
        }"#;
        let cfg: AppConfig = serde_json::from_str(&config_json_with_mirror(mirror)).unwrap();
        assert!(!cfg.config_sync.enabled);
        assert_eq!(cfg.config_sync.interval_seconds, 300);
    }

    #[test]
    fn test_config_sync_url_parsed() {
        let json = r#"{
            "host": "0.0.0.0", "port": 7878, "publicOrigin": "https://example.com",
            "trustProxyHeaders": true, "logLevel": "info",
            "geo": { "mode": "off", "headerName": "h", "countries": [] },
            "cacheTTL": { "raw": 0, "avatar": 0, "unpkg": 0 },
            "mirror": { "defaultTTL": 0, "defaultMaxSize": 1, "absoluteMaxSize": 1, "fetchTimeoutMs": 1 },
            "cors": { "enabledRoutes": [] },
            "auth": { "enabled": false, "key": "", "value": "" },
            "configSync": { "enabled": true, "intervalSeconds": 60, "url": "https://example.com/config.mirror.json" }
        }"#;
        let cfg: AppConfig = serde_json::from_str(json).unwrap();
        assert!(cfg.config_sync.enabled);
        assert_eq!(cfg.config_sync.interval_seconds, 60);
        assert_eq!(cfg.config_sync.url, "https://example.com/config.mirror.json");
    }

    // ── merged whitelists ────────────────────────────────────────────────

    #[test]
    fn test_whitelists_default_when_absent() {
        // A document without a `whitelists` key yields empty whitelists.
        let mirror = r#"{
            "defaultTTL": 0, "defaultMaxSize": 1, "absoluteMaxSize": 1, "fetchTimeoutMs": 1
        }"#;
        let cfg: AppConfig = serde_json::from_str(&config_json_with_mirror(mirror)).unwrap();
        assert!(cfg.whitelists.avatar.is_empty());
        assert!(cfg.whitelists.raw.is_empty());
    }

    #[test]
    fn test_parse_config_reads_merged_whitelists() {
        let json = r#"{
            "host": "0.0.0.0", "port": 7878, "publicOrigin": "https://example.com",
            "trustProxyHeaders": true, "logLevel": "info",
            "geo": { "mode": "off", "headerName": "h", "countries": [] },
            "cacheTTL": { "raw": 0, "avatar": 0, "unpkg": 0 },
            "mirror": { "defaultTTL": 0, "defaultMaxSize": 1, "absoluteMaxSize": 1, "fetchTimeoutMs": 1 },
            "cors": { "enabledRoutes": [] },
            "auth": { "enabled": false, "key": "", "value": "" },
            "whitelists": {
                "avatar": ["karinjs"],
                "raw": { "karinjs": { "karin": [{ "branch": "HEAD", "file": "package.json" }] } },
                "unpkg": { "karin": ["package.json"] }
            }
        }"#;
        let cfg = parse_config(json).unwrap();
        assert_eq!(cfg.whitelists.avatar, vec!["karinjs"]);
        assert!(cfg.whitelists.raw.contains_key("karinjs"));
        assert!(cfg.whitelists.unpkg.contains_key("karin"));
        // omitted sections default to empty
        assert!(cfg.whitelists.releases.is_empty());
        assert!(cfg.whitelists.mirror.is_empty());
    }

    // ── validate_config ──────────────────────────────────────────────────

    #[test]
    fn test_validate_config_ok() {
        let mirror = r#"{
            "defaultTTL": 0,
            "defaultMaxSize": 52428800,
            "absoluteMaxSize": 1073741824,
            "fetchTimeoutMs": 30000
        }"#;
        let cfg: AppConfig = serde_json::from_str(&config_json_with_mirror(mirror)).unwrap();
        assert!(validate_config(&cfg).is_ok());
    }

    #[test]
    fn test_validate_config_default_max_size_exceeds_absolute() {
        let mirror = r#"{
            "defaultTTL": 0,
            "defaultMaxSize": 1073741825,
            "absoluteMaxSize": 1073741824,
            "fetchTimeoutMs": 30000
        }"#;
        let cfg: AppConfig = serde_json::from_str(&config_json_with_mirror(mirror)).unwrap();
        let err = validate_config(&cfg).unwrap_err();
        assert!(err.to_string().contains("absoluteMaxSize"));
    }

    #[test]
    fn test_validate_config_max_size_equal_is_ok() {
        let mirror = r#"{
            "defaultTTL": 0,
            "defaultMaxSize": 1024,
            "absoluteMaxSize": 1024,
            "fetchTimeoutMs": 30000
        }"#;
        let cfg: AppConfig = serde_json::from_str(&config_json_with_mirror(mirror)).unwrap();
        assert!(validate_config(&cfg).is_ok());
    }

    #[test]
    fn test_validate_config_ttl_sentinels_allowed() {
        // -2, -1, 0 are valid sentinel TTL values.
        let json = r#"{
            "host": "0.0.0.0", "port": 7878, "publicOrigin": "https://example.com",
            "trustProxyHeaders": true, "logLevel": "info",
            "geo": { "mode": "off", "headerName": "h", "countries": [] },
            "cacheTTL": { "raw": -2, "avatar": -1, "unpkg": 0 },
            "mirror": { "defaultTTL": 0, "defaultMaxSize": 1, "absoluteMaxSize": 1, "fetchTimeoutMs": 1 },
            "cors": { "enabledRoutes": [] },
            "auth": { "enabled": false, "key": "", "value": "" }
        }"#;
        let cfg: AppConfig = serde_json::from_str(json).unwrap();
        assert!(validate_config(&cfg).is_ok());
    }

    #[test]
    fn test_validate_config_ttl_below_minus_two_rejected() {
        let json = r#"{
            "host": "0.0.0.0", "port": 7878, "publicOrigin": "https://example.com",
            "trustProxyHeaders": true, "logLevel": "info",
            "geo": { "mode": "off", "headerName": "h", "countries": [] },
            "cacheTTL": { "raw": -3, "avatar": 0, "unpkg": 0 },
            "mirror": { "defaultTTL": 0, "defaultMaxSize": 1, "absoluteMaxSize": 1, "fetchTimeoutMs": 1 },
            "cors": { "enabledRoutes": [] },
            "auth": { "enabled": false, "key": "", "value": "" }
        }"#;
        let cfg: AppConfig = serde_json::from_str(json).unwrap();
        let err = validate_config(&cfg).unwrap_err();
        assert!(err.to_string().contains("cacheTTL.raw"));
    }

    #[test]
    fn test_unknown_field_rejected() {
        // deny_unknown_fields means a stray top-level key fails deserialization.
        let mirror = r#"{
            "defaultTTL": 0, "defaultMaxSize": 1, "absoluteMaxSize": 1, "fetchTimeoutMs": 1
        }"#;
        let mut json: serde_json::Value =
            serde_json::from_str(&config_json_with_mirror(mirror)).unwrap();
        json["bogusField"] = serde_json::json!(true);
        let result: std::result::Result<AppConfig, _> = serde_json::from_value(json);
        assert!(result.is_err());
    }
}
