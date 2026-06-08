//! Tencent EdgeOne "origin protection" (源站保护) auto-pull.
//!
//! When `originProtection.enabled` is set, this periodically calls the EO
//! `DescribeOriginACL` API for the configured zone, extracts EO's back-to-origin
//! IP ranges, and installs an nftables ruleset so only EO 回源 (plus loopback)
//! can reach the guarded ports. EO recommends polling roughly every 3 days.
//!
//! Requirements: the process must run as root on Linux with `nft` available.
//! The feature is off by default; credentials live in the (gitignored) config.

use crate::config::{AppState, OriginProtectionConfig};
use hmac::{Hmac, Mac};
use reqwest::Client;
use sha2::{Digest, Sha256};
use std::net::{Ipv4Addr, Ipv6Addr};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const API_HOST: &str = "teo.tencentcloudapi.com";
const API_ACTION: &str = "DescribeOriginACL";
const API_VERSION: &str = "2022-09-01";
const NFT_TABLE: &str = "origin_guard";

type HmacSha256 = Hmac<Sha256>;

/// EO back-to-origin IP ranges, already validated as CIDRs.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct OriginAcl {
    pub v4: Vec<String>,
    pub v6: Vec<String>,
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn sha256_hex(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

fn hmac_sha256(key: &[u8], msg: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(msg);
    mac.finalize().into_bytes().to_vec()
}

/// Convert a unix timestamp (seconds) to a UTC `YYYY-MM-DD` string.
///
/// Uses Howard Hinnant's civil-from-days algorithm so we don't pull in a date
/// crate. Only valid for non-negative timestamps (all real ones).
fn utc_date(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as i64;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Build the `Authorization` header value using Tencent Cloud's TC3-HMAC-SHA256
/// signature scheme. `action_lower` must be the lowercased action name.
fn build_authorization(
    secret_id: &str,
    secret_key: &str,
    action_lower: &str,
    payload: &str,
    timestamp: u64,
    date: &str,
) -> String {
    let canonical_headers = format!(
        "content-type:application/json; charset=utf-8\nhost:{API_HOST}\nx-tc-action:{action_lower}\n"
    );
    let signed_headers = "content-type;host;x-tc-action";
    let hashed_payload = sha256_hex(payload.as_bytes());
    let canonical_request =
        format!("POST\n/\n\n{canonical_headers}\n{signed_headers}\n{hashed_payload}");

    let credential_scope = format!("{date}/teo/tc3_request");
    let string_to_sign = format!(
        "TC3-HMAC-SHA256\n{timestamp}\n{credential_scope}\n{}",
        sha256_hex(canonical_request.as_bytes())
    );

    let secret_date = hmac_sha256(format!("TC3{secret_key}").as_bytes(), date.as_bytes());
    let secret_service = hmac_sha256(&secret_date, b"teo");
    let secret_signing = hmac_sha256(&secret_service, b"tc3_request");
    let signature = hex(&hmac_sha256(&secret_signing, string_to_sign.as_bytes()));

    format!(
        "TC3-HMAC-SHA256 Credential={secret_id}/{credential_scope}, \
         SignedHeaders={signed_headers}, Signature={signature}"
    )
}

/// A zone id must look like `zone-xxxx` (lowercase alphanumerics + dashes) so it
/// can be embedded in the JSON payload without escaping concerns.
fn valid_zone_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn is_cidr_v4(s: &str) -> bool {
    match s.split_once('/') {
        Some((ip, len)) => ip.parse::<Ipv4Addr>().is_ok() && matches!(len.parse::<u8>(), Ok(n) if n <= 32),
        None => false,
    }
}

fn is_cidr_v6(s: &str) -> bool {
    match s.split_once('/') {
        Some((ip, len)) => ip.parse::<Ipv6Addr>().is_ok() && matches!(len.parse::<u8>(), Ok(n) if n <= 128),
        None => false,
    }
}

/// Collect valid CIDR strings from a JSON array value, dropping (and logging)
/// anything that isn't a well-formed CIDR. Strict validation also guarantees the
/// strings are safe to splice into the nft ruleset (no metacharacters).
fn collect_cidrs(arr: Option<&serde_json::Value>, validate: fn(&str) -> bool) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(items) = arr.and_then(|v| v.as_array()) {
        for item in items {
            if let Some(s) = item.as_str() {
                if validate(s) {
                    out.push(s.to_string());
                } else {
                    tracing::warn!("origin protection: ignoring invalid CIDR {s:?}");
                }
            }
        }
    }
    out
}

/// Parse a `DescribeOriginACL` response body into validated CIDR lists.
///
/// Targets `Response.OriginACLInfo.CurrentOriginACL.EntireAddresses.{IPv4,IPv6}`
/// with a fallback to `OriginACLInfo.EntireAddresses`.
pub fn parse_origin_acl(body: &str) -> Result<OriginAcl, String> {
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("invalid JSON: {e}"))?;

    if let Some(err) = v.pointer("/Response/Error") {
        return Err(format!("EO API error: {err}"));
    }

    let info = v
        .pointer("/Response/OriginACLInfo")
        .ok_or("missing Response.OriginACLInfo")?;

    let entire = info
        .get("CurrentOriginACL")
        .and_then(|c| c.get("EntireAddresses"))
        .or_else(|| info.get("EntireAddresses"))
        .ok_or("missing EntireAddresses")?;

    let v4 = collect_cidrs(entire.get("IPv4"), is_cidr_v4);
    let v6 = collect_cidrs(entire.get("IPv6"), is_cidr_v6);

    if v4.is_empty() && v6.is_empty() {
        return Err("response contained no valid CIDRs".to_string());
    }
    Ok(OriginAcl { v4, v6 })
}

async fn fetch_origin_acl(client: &Client, cfg: &OriginProtectionConfig) -> Result<OriginAcl, String> {
    let payload = format!("{{\"ZoneId\":\"{}\"}}", cfg.zone_id);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("clock error: {e}"))?
        .as_secs();
    let date = utc_date(timestamp);
    let auth = build_authorization(
        &cfg.secret_id,
        &cfg.secret_key,
        &API_ACTION.to_ascii_lowercase(),
        &payload,
        timestamp,
        &date,
    );

    let resp = client
        .post(format!("https://{API_HOST}/"))
        .header("Authorization", auth)
        .header("Content-Type", "application/json; charset=utf-8")
        .header("Host", API_HOST)
        .header("X-TC-Action", API_ACTION)
        .header("X-TC-Timestamp", timestamp.to_string())
        .header("X-TC-Version", API_VERSION)
        .timeout(Duration::from_secs(20))
        .body(payload)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    let status = resp.status();
    let body = resp.text().await.map_err(|e| format!("read body: {e}"))?;
    if !status.is_success() {
        return Err(format!("HTTP {status}: {body}"));
    }
    parse_origin_acl(&body)
}

/// Render an atomic nftables ruleset that allows only EO ranges (+ loopback +
/// established) to the guarded ports, dropping everything else to those ports.
/// Other ports (SSH, etc.) are untouched.
pub fn build_nft_ruleset(table: &str, ports: &[u16], acl: &OriginAcl) -> String {
    let port_list = ports
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(", ");

    let mut s = String::new();
    // Atomic replace: ensure-exists, delete, recreate.
    s.push_str(&format!("add table inet {table}\n"));
    s.push_str(&format!("delete table inet {table}\n"));
    s.push_str(&format!("add table inet {table}\n"));
    s.push_str(&format!(
        "add set inet {table} eo_v4 {{ type ipv4_addr; flags interval; }}\n"
    ));
    s.push_str(&format!(
        "add set inet {table} eo_v6 {{ type ipv6_addr; flags interval; }}\n"
    ));
    if !acl.v4.is_empty() {
        s.push_str(&format!(
            "add element inet {table} eo_v4 {{ {} }}\n",
            acl.v4.join(", ")
        ));
    }
    if !acl.v6.is_empty() {
        s.push_str(&format!(
            "add element inet {table} eo_v6 {{ {} }}\n",
            acl.v6.join(", ")
        ));
    }
    s.push_str(&format!(
        "add chain inet {table} input {{ type filter hook input priority -10; policy accept; }}\n"
    ));
    s.push_str(&format!("add rule inet {table} input iif \"lo\" accept\n"));
    s.push_str(&format!(
        "add rule inet {table} input ct state established,related accept\n"
    ));
    s.push_str(&format!(
        "add rule inet {table} input tcp dport {{ {port_list} }} ip saddr @eo_v4 accept\n"
    ));
    s.push_str(&format!(
        "add rule inet {table} input tcp dport {{ {port_list} }} ip6 saddr @eo_v6 accept\n"
    ));
    s.push_str(&format!(
        "add rule inet {table} input tcp dport {{ {port_list} }} drop\n"
    ));
    s
}

#[cfg(target_os = "linux")]
fn apply_nft(ruleset: &str) -> Result<(), String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new("nft")
        .arg("-f")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn nft: {e}"))?;
    child
        .stdin
        .take()
        .ok_or("nft stdin unavailable")?
        .write_all(ruleset.as_bytes())
        .map_err(|e| format!("write nft stdin: {e}"))?;
    let out = child.wait_with_output().map_err(|e| format!("wait nft: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "nft exited {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn apply_nft(_ruleset: &str) -> Result<(), String> {
    Err("nftables firewall is only supported on Linux".to_string())
}

pub async fn origin_protection_task(state: AppState, client: Client) {
    loop {
        let cfg = { state.config.read().await.origin_protection.clone() };
        let interval = if cfg.interval_seconds == 0 {
            259_200
        } else {
            cfg.interval_seconds
        };

        if cfg.enabled {
            run_once(&client, &cfg).await;
        } else {
            tracing::debug!("origin protection: disabled");
        }

        tokio::time::sleep(Duration::from_secs(interval)).await;
    }
}

async fn run_once(client: &Client, cfg: &OriginProtectionConfig) {
    if cfg.secret_id.is_empty() || cfg.secret_key.is_empty() || cfg.zone_id.is_empty() {
        tracing::warn!("origin protection: enabled but zoneId/secretId/secretKey missing — skipping");
        return;
    }
    if !valid_zone_id(&cfg.zone_id) {
        tracing::warn!("origin protection: zoneId {:?} is malformed — skipping", cfg.zone_id);
        return;
    }
    if cfg.ports.is_empty() {
        tracing::warn!("origin protection: no ports configured — skipping");
        return;
    }

    match fetch_origin_acl(client, cfg).await {
        Ok(acl) => {
            let ruleset = build_nft_ruleset(NFT_TABLE, &cfg.ports, &acl);
            match apply_nft(&ruleset) {
                Ok(()) => tracing::info!(
                    "origin protection: applied {} IPv4 + {} IPv6 EO ranges to ports {:?}",
                    acl.v4.len(),
                    acl.v6.len(),
                    cfg.ports
                ),
                Err(e) => tracing::warn!("origin protection: nft apply failed — {e}"),
            }
        }
        Err(e) => tracing::warn!("origin protection: fetch failed — {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── hmac / hex (RFC 4231 test case 2) ──

    #[test]
    fn test_hmac_sha256_rfc4231() {
        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(
            hex(&mac),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn test_sha256_hex_empty() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    // ── utc_date ──

    #[test]
    fn test_utc_date_known() {
        assert_eq!(utc_date(0), "1970-01-01");
        assert_eq!(utc_date(86_400), "1970-01-02");
        assert_eq!(utc_date(1_735_689_600), "2025-01-01");
        assert_eq!(utc_date(1_751_000_000), "2025-06-27");
    }

    // ── TC3 signature structure (crypto correctness covered by RFC4231 test) ──

    #[test]
    fn test_build_authorization_shape_and_determinism() {
        let a = build_authorization("AKIDxxxx", "secret", "describeoriginacl", "{\"ZoneId\":\"zone-1\"}", 1_700_000_000, "2023-11-14");
        let b = build_authorization("AKIDxxxx", "secret", "describeoriginacl", "{\"ZoneId\":\"zone-1\"}", 1_700_000_000, "2023-11-14");
        assert_eq!(a, b, "signing must be deterministic");
        assert!(a.starts_with("TC3-HMAC-SHA256 Credential=AKIDxxxx/2023-11-14/teo/tc3_request"));
        assert!(a.contains("SignedHeaders=content-type;host;x-tc-action"));
        assert!(a.contains("Signature="));
        // Different secret -> different signature.
        let c = build_authorization("AKIDxxxx", "other", "describeoriginacl", "{\"ZoneId\":\"zone-1\"}", 1_700_000_000, "2023-11-14");
        assert_ne!(a, c);
    }

    // ── zone id / cidr validation ──

    #[test]
    fn test_valid_zone_id() {
        assert!(valid_zone_id("zone-276zs184g93m"));
        assert!(!valid_zone_id(""));
        assert!(!valid_zone_id("zone with space"));
        assert!(!valid_zone_id("zone\"; rm -rf"));
        assert!(!valid_zone_id("ZONE-UPPER"));
    }

    #[test]
    fn test_cidr_validation() {
        assert!(is_cidr_v4("1.2.3.0/24"));
        assert!(is_cidr_v4("203.0.113.5/32"));
        assert!(!is_cidr_v4("1.2.3.0/33"));
        assert!(!is_cidr_v4("1.2.3.0"));
        assert!(!is_cidr_v4("not-an-ip/24"));
        assert!(is_cidr_v6("2402:4e00::/32"));
        assert!(!is_cidr_v6("2402:4e00::/129"));
        assert!(!is_cidr_v6("1.2.3.0/24"));
    }

    // ── response parsing ──

    #[test]
    fn test_parse_origin_acl_current() {
        let body = r#"{"Response":{"OriginACLInfo":{"CurrentOriginACL":{"EntireAddresses":{"IPv4":["1.2.3.0/24","5.6.7.8/32"],"IPv6":["2402:4e00::/32"]}}},"RequestId":"x"}}"#;
        let acl = parse_origin_acl(body).unwrap();
        assert_eq!(acl.v4, vec!["1.2.3.0/24", "5.6.7.8/32"]);
        assert_eq!(acl.v6, vec!["2402:4e00::/32"]);
    }

    #[test]
    fn test_parse_origin_acl_fallback_and_filter() {
        // EntireAddresses directly under OriginACLInfo, with one bogus entry filtered.
        let body = r#"{"Response":{"OriginACLInfo":{"EntireAddresses":{"IPv4":["9.9.9.0/24","garbage"]}}}}"#;
        let acl = parse_origin_acl(body).unwrap();
        assert_eq!(acl.v4, vec!["9.9.9.0/24"]);
        assert!(acl.v6.is_empty());
    }

    #[test]
    fn test_parse_origin_acl_api_error() {
        let body = r#"{"Response":{"Error":{"Code":"AuthFailure","Message":"bad key"},"RequestId":"x"}}"#;
        assert!(parse_origin_acl(body).unwrap_err().contains("EO API error"));
    }

    #[test]
    fn test_parse_origin_acl_empty_rejected() {
        let body = r#"{"Response":{"OriginACLInfo":{"CurrentOriginACL":{"EntireAddresses":{"IPv4":[],"IPv6":[]}}}}}"#;
        assert!(parse_origin_acl(body).is_err());
    }

    // ── nft ruleset generation ──

    #[test]
    fn test_build_nft_ruleset() {
        let acl = OriginAcl {
            v4: vec!["1.2.3.0/24".into()],
            v6: vec!["2402:4e00::/32".into()],
        };
        let rs = build_nft_ruleset("origin_guard", &[80, 443], &acl);
        assert!(rs.contains("add table inet origin_guard"));
        assert!(rs.contains("delete table inet origin_guard"));
        assert!(rs.contains("add element inet origin_guard eo_v4 { 1.2.3.0/24 }"));
        assert!(rs.contains("add element inet origin_guard eo_v6 { 2402:4e00::/32 }"));
        assert!(rs.contains("tcp dport { 80, 443 } ip saddr @eo_v4 accept"));
        assert!(rs.contains("tcp dport { 80, 443 } ip6 saddr @eo_v6 accept"));
        assert!(rs.contains("tcp dport { 80, 443 } drop"));
        assert!(rs.contains("iif \"lo\" accept"));
    }

    #[test]
    fn test_build_nft_ruleset_skips_empty_v6() {
        let acl = OriginAcl { v4: vec!["1.2.3.0/24".into()], v6: vec![] };
        let rs = build_nft_ruleset("origin_guard", &[80], &acl);
        assert!(rs.contains("add element inet origin_guard eo_v4"));
        assert!(!rs.contains("add element inet origin_guard eo_v6"));
    }
}
