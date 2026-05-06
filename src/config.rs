use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::Result;
use tracing::debug;

#[derive(Debug, Clone, Deserialize, Serialize)]
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
pub struct MirrorRuntimeConfig {
    #[serde(rename = "defaultTTL")]
    pub default_ttl: i32,
    #[serde(rename = "defaultMaxSize")]
    pub default_max_size: usize,
    #[serde(rename = "absoluteMaxSize")]
    pub absolute_max_size: usize,
    #[serde(rename = "fetchTimeoutMs")]
    pub fetch_timeout_ms: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RouteCacheTTLConfig {
    pub raw: i32,
    pub avatar: i32,
    pub unpkg: i32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CorsConfig {
    #[serde(rename = "enabledRoutes")]
    pub enabled_routes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthConfig {
    pub enabled: bool,
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ConfigSyncUrls {
    #[serde(default)]
    pub avatar: String,
    #[serde(default)]
    pub raw: String,
    #[serde(default)]
    pub releases: String,
    #[serde(default)]
    pub mirror: String,
    #[serde(default)]
    pub unpkg: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConfigSyncConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(rename = "intervalSeconds", default = "default_sync_interval")]
    pub interval_seconds: u64,
    #[serde(default)]
    pub urls: ConfigSyncUrls,
}

fn default_sync_interval() -> u64 { 300 }

impl Default for ConfigSyncConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_seconds: default_sync_interval(),
            urls: ConfigSyncUrls::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
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

#[derive(Debug, Clone)]
pub struct Whitelists {
    pub avatar: AvatarWhitelist,
    pub raw: RawWhitelist,
    pub releases: ReleasesWhitelist,
    pub unpkg: UnpkgWhitelist,
    pub mirror: MirrorWhitelist,
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<AppConfig>>,
    pub whitelists: Arc<RwLock<Whitelists>>,
}

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
            "fetchTimeoutMs": 30000
        },
        "cors": { "enabledRoutes": ["raw", "unpkg", "mirror"] },
        "auth": { "enabled": false, "key": "", "value": "" },
        "configSync": {
            "enabled": false,
            "intervalSeconds": 300,
            "urls": {
                "avatar": "",
                "raw": "",
                "releases": "",
                "mirror": "",
                "unpkg": ""
            }
        }
    });

    let files: &[(&str, serde_json::Value)] = &[
        ("config.json", config),
        ("github.avatar.json", serde_json::json!(["karinjs"])),
        ("github.raw.json", serde_json::json!({
            "karinjs": {
                "karin": [{"branch": "HEAD", "file": "package.json"}]
            }
        })),
        ("github.releases.json", serde_json::json!({
            "NapNeko": {
                "NapCatQQ": ["NapCat.Framework.zip"]
            }
        })),
        ("unpkg.json", serde_json::json!({
            "karin": ["package.json", "dist/karin.umd.js"]
        })),
        ("mirror.json", serde_json::json!({
            "https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions.json": 0
        })),
    ];

    for (name, content) in files {
        let text = serde_json::to_string_pretty(content)?;
        tokio::fs::write(config_root.join(name), text).await?;
    }
    debug!("default config generated");

    Ok(())
}

impl AppState {
    pub async fn load() -> Result<Self> {
        let config_root = std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join("config");

        if !config_root.is_dir() {
            generate_defaults(&config_root).await?;
        }

        let config: AppConfig = serde_json::from_str(
            &tokio::fs::read_to_string(config_root.join("config.json")).await?
        )?;

        let avatar: AvatarWhitelist = serde_json::from_str(
            &tokio::fs::read_to_string(config_root.join("github.avatar.json")).await?
        )?;

        let raw: RawWhitelist = serde_json::from_str(
            &tokio::fs::read_to_string(config_root.join("github.raw.json")).await?
        )?;

        let releases: ReleasesWhitelist = serde_json::from_str(
            &tokio::fs::read_to_string(config_root.join("github.releases.json")).await?
        )?;

        let unpkg: UnpkgWhitelist = serde_json::from_str(
            &tokio::fs::read_to_string(config_root.join("unpkg.json")).await?
        )?;

        let mirror: MirrorWhitelist = serde_json::from_str(
            &tokio::fs::read_to_string(config_root.join("mirror.json")).await?
        )?;

        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            whitelists: Arc::new(RwLock::new(Whitelists {
                avatar,
                raw,
                releases,
                unpkg,
                mirror,
            })),
        })
    }
}
