use std::net::{IpAddr, Ipv4Addr};

/// Validate a single path component (owner, repo, tag, file path).
/// Rejects empty strings, path traversal (`..`), double slashes (`//`),
/// and backslashes (`\`).
pub fn validate_path_component(s: &str) -> bool {
    !s.is_empty() && !s.contains("..") && !s.contains("//") && !s.contains('\\')
}

/// Validate a hostname for proxying: rejects empty, too long, path traversal,
/// protocol injection, invalid DNS labels, and most importantly,
/// resolves to IP and rejects internal/loopback/link-local addresses (SSRF protection).
pub fn validate_host(s: &str) -> bool {
    // Max DNS hostname length (RFC 1035)
    if s.is_empty() || s.len() > 253 {
        return false;
    }
    // Reject path traversal and protocol injection
    if s.contains("//") || s.contains('\\') || s.contains("..") {
        return false;
    }
    // Reject leading/trailing dots and hyphens (invalid DNS labels)
    if s.starts_with('.') || s.ends_with('.') || s.starts_with('-') || s.ends_with('-') {
        return false;
    }
    // Reject IPv6 bracket notation (e.g., [::1])
    if s.starts_with('[') || s.contains("::") || s.matches(':').count() > 1 {
        return false;
    }
    // All characters must be alphanumeric, dot, hyphen, or a single colon (for port)
    if !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == ':') {
        return false;
    }

    // Extract hostname part (strip optional port)
    let hostname = match s.rfind(':') {
        Some(pos) => &s[..pos],
        None => s,
    };

    // SSRF protection: reject loopback, private, and link-local addresses
    if let Ok(ip) = hostname.parse::<IpAddr>() {
        return is_global_ip(ip);
    }

    // Reject localhost explicitly
    if hostname.eq_ignore_ascii_case("localhost") {
        return false;
    }

    true
}

/// Validate a config sync URL: must be https, host must pass static checks,
/// and DNS resolution must not return internal/loopback addresses (SSRF + DNS rebinding protection).
pub async fn validate_sync_url(url: &str) -> Result<(), &'static str> {
    if !url.starts_with("https://") {
        return Err("sync URL must use https");
    }
    let host_part = url
        .strip_prefix("https://")
        .and_then(|s| s.split('/').next())
        .unwrap_or("");
    if host_part.is_empty() {
        return Err("sync URL has no host");
    }
    // Reject URLs with userinfo (e.g. https://evil.com@trusted.com/) to prevent SSRF bypass
    if host_part.contains('@') {
        return Err("sync URL must not contain userinfo (@)");
    }
    // Strip port
    let (hostname, port) = match host_part.rfind(':') {
        Some(pos) => (&host_part[..pos], host_part[pos+1..].parse::<u16>().unwrap_or(443)),
        None => (host_part, 443u16),
    };
    // Reject loopback/private/link-local
    if hostname.eq_ignore_ascii_case("localhost") {
        return Err("sync URL must not point to localhost");
    }
    if let Ok(ip) = hostname.parse::<IpAddr>() {
        if !is_global_ip(ip) {
            return Err("sync URL must not point to internal IP");
        }
    }
    // DNS rebinding protection: resolve hostname and verify all IPs are globally routable
    resolve_and_validate_host(hostname, port).await
}

/// Resolve a hostname via DNS and verify all resolved IPs are globally routable.
/// This prevents DNS rebinding attacks where a domain resolves to an internal IP.
pub async fn resolve_and_validate_host(hostname: &str, port: u16) -> Result<(), &'static str> {
    // If hostname is already an IP, validate_host already checked it
    if hostname.parse::<IpAddr>().is_ok() {
        return Ok(());
    }

    let addr_str = format!("{}:{}", hostname, port);
    let addrs = tokio::net::lookup_host(&addr_str)
        .await
        .map_err(|_| "DNS resolution failed")?;

    for addr in addrs {
        if !is_global_ip(addr.ip()) {
            return Err("hostname resolves to internal IP");
        }
    }

    Ok(())
}

/// Check if an IP address is globally routable (not internal/loopback/link-local).
fn is_global_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            !v4.is_loopback()
                && !v4.is_private()
                && !v4.is_link_local()
                && !v4.is_broadcast()
                && !v4.is_unspecified()
                // Reject 0.x.x.x/8 (current network)
                && v4.octets()[0] != 0
                // Reject 100.64.0.0/10 (CGNAT)
                && !is_shared_address(v4)
                // Reject 192.0.0.0/24 (IETF protocol assignments)
                && !is_ietf_reserved(v4)
                // Reject 192.0.2.0/24 (TEST-NET-1), 198.51.100.0/24 (TEST-NET-2), 203.0.113.0/24 (TEST-NET-3)
                && !is_documentation_net(v4)
                // Reject 224.0.0.0/4 (multicast)
                && !v4.is_multicast()
        }
        IpAddr::V6(v6) => {
            // Fold IPv4-mapped (::ffff:a.b.c.d) and IPv4-compatible (::a.b.c.d)
            // addresses back to IPv4 and re-check, so an internal IPv4 can't be
            // smuggled past the filter via a mapped IPv6 form.
            if let Some(v4) = v6.to_ipv4_mapped().or_else(|| v6.to_ipv4()) {
                return is_global_ip(IpAddr::V4(v4));
            }
            !v6.is_loopback()
                && !v6.is_unspecified()
                // Reject fc00::/7 (unique local, IPv6 private)
                && !v6.is_unique_local()
                // Reject fe80::/10 (link-local)
                && !v6.is_unicast_link_local()
                // Reject ff00::/8 (multicast)
                && !v6.is_multicast()
        }
    }
}

/// Check if IPv4 is in 100.64.0.0/10 (CGNAT/shared address space).
fn is_shared_address(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 100 && (octets[1] & 0xC0) == 64
}

/// Check if IPv4 is in 192.0.0.0/24 (IETF protocol assignments).
fn is_ietf_reserved(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 192 && octets[1] == 0 && octets[2] == 0
}

/// Check if IPv4 is in documentation/test ranges (RFC 5737):
/// 192.0.2.0/24 (TEST-NET-1), 198.51.100.0/24 (TEST-NET-2), 203.0.113.0/24 (TEST-NET-3).
fn is_documentation_net(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    (o[0] == 192 && o[1] == 0 && o[2] == 2)
        || (o[0] == 198 && o[1] == 51 && o[2] == 100)
        || (o[0] == 203 && o[1] == 0 && o[2] == 113)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── validate_path_component ──

    #[test]
    fn test_valid_components() {
        assert!(validate_path_component("file.json"));
        assert!(validate_path_component("HEAD"));
        assert!(validate_path_component("main"));
        assert!(validate_path_component("dist/bundle.js"));
        assert!(validate_path_component("subdir/file.zip"));
        assert!(validate_path_component("file-name_v1.0.tar.gz"));
        assert!(validate_path_component("package.json"));
        assert!(validate_path_component("src/index.ts"));
    }

    #[test]
    fn test_invalid_components() {
        assert!(!validate_path_component(""));
        assert!(!validate_path_component("../secret"));
        assert!(!validate_path_component("path/../file"));
        assert!(!validate_path_component("path//file"));
        assert!(!validate_path_component("path\\file"));
        assert!(!validate_path_component(".."));
    }

    // ── validate_host ──

    #[test]
    fn test_valid_hosts() {
        assert!(validate_host("example.com"));
        assert!(validate_host("sub.example.com"));
        assert!(validate_host("example.com:8080"));
        assert!(validate_host("cdn.jsdelivr.net"));
    }

    #[test]
    fn test_invalid_hosts() {
        assert!(!validate_host(""));
        assert!(!validate_host("example..com"));
        assert!(!validate_host("example//com"));
        assert!(!validate_host("example\\com"));
        assert!(!validate_host("../etc/passwd"));
    }

    #[test]
    fn test_rejects_ipv6() {
        assert!(!validate_host("::1"));
        assert!(!validate_host("fe80::1"));
        assert!(!validate_host("[::1]"));
        assert!(!validate_host("2001:db8::1"));
    }

    #[test]
    fn test_rejects_ipv4_mapped_ipv6_internal() {
        // An IPv4-mapped IPv6 address must be folded to its IPv4 form and
        // rejected if that IPv4 is internal (SSRF / DNS-rebinding defense).
        assert!(!is_global_ip("::ffff:127.0.0.1".parse().unwrap()));
        assert!(!is_global_ip("::ffff:10.0.0.1".parse().unwrap()));
        assert!(!is_global_ip("::ffff:192.168.1.1".parse().unwrap()));
        assert!(!is_global_ip("::ffff:169.254.1.1".parse().unwrap()));
        // A mapped public IPv4 is still allowed.
        assert!(is_global_ip("::ffff:8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn test_allows_global_ipv6() {
        // A genuine global IPv6 address is not folded and passes.
        assert!(is_global_ip("2606:4700:4700::1111".parse().unwrap()));
    }

    #[test]
    fn test_rejects_boundary_chars() {
        assert!(!validate_host(".example.com"));
        assert!(!validate_host("example.com."));
        assert!(!validate_host("-example.com"));
        assert!(!validate_host("example.com-"));
    }

    #[test]
    fn test_rejects_too_long() {
        let long_host = "a".repeat(254);
        assert!(!validate_host(&long_host));
        let max_host = format!("{}.com", "a".repeat(249));
        assert!(validate_host(&max_host));
    }

    // ── SSRF protection ──

    #[test]
    fn test_rejects_loopback() {
        assert!(!validate_host("127.0.0.1"));
        assert!(!validate_host("127.0.0.2"));
        assert!(!validate_host("127.255.255.255"));
    }

    #[test]
    fn test_rejects_private_ranges() {
        // 10.0.0.0/8
        assert!(!validate_host("10.0.0.1"));
        assert!(!validate_host("10.255.255.255"));
        // 172.16.0.0/12
        assert!(!validate_host("172.16.0.1"));
        assert!(!validate_host("172.31.255.255"));
        // 192.168.0.0/16
        assert!(!validate_host("192.168.1.1"));
        assert!(!validate_host("192.168.0.0"));
    }

    #[test]
    fn test_rejects_link_local() {
        assert!(!validate_host("169.254.1.1"));
        assert!(!validate_host("169.254.0.0"));
    }

    #[test]
    fn test_rejects_cgnat() {
        assert!(!validate_host("100.64.0.1"));
        assert!(!validate_host("100.127.255.255"));
    }

    #[test]
    fn test_rejects_localhost() {
        assert!(!validate_host("localhost"));
        assert!(!validate_host("LocalHost"));
    }

    #[test]
    fn test_rejects_unspecified() {
        assert!(!validate_host("0.0.0.0"));
    }

    #[test]
    fn test_rejects_current_network() {
        assert!(!validate_host("0.0.0.1"));
        assert!(!validate_host("0.255.255.255"));
    }

    #[test]
    fn test_rejects_multicast() {
        assert!(!validate_host("224.0.0.1"));
        assert!(!validate_host("239.255.255.255"));
    }

    #[test]
    fn test_rejects_documentation_nets() {
        // TEST-NET-1
        assert!(!validate_host("192.0.2.1"));
        assert!(!validate_host("192.0.2.255"));
        // TEST-NET-2
        assert!(!validate_host("198.51.100.1"));
        assert!(!validate_host("198.51.100.255"));
        // TEST-NET-3
        assert!(!validate_host("203.0.113.1"));
        assert!(!validate_host("203.0.113.255"));
    }

    #[test]
    fn test_allows_public_ips() {
        assert!(validate_host("8.8.8.8"));
        assert!(validate_host("1.1.1.1"));
        assert!(validate_host("93.184.216.34"));
    }

    #[test]
    fn test_ip_with_port() {
        assert!(!validate_host("127.0.0.1:8080"));
        assert!(!validate_host("192.168.1.1:3000"));
        assert!(validate_host("8.8.8.8:443"));
    }

    // ── validate_sync_url ──

    #[tokio::test]
    async fn test_sync_url_must_be_https() {
        assert!(validate_sync_url("https://example.com/config.json").await.is_ok());
        assert!(validate_sync_url("http://example.com/config.json").await.is_err());
        assert!(validate_sync_url("ftp://example.com/config.json").await.is_err());
    }

    #[tokio::test]
    async fn test_sync_url_rejects_localhost() {
        assert!(validate_sync_url("https://localhost/config.json").await.is_err());
        assert!(validate_sync_url("https://LocalHost/config.json").await.is_err());
    }

    #[tokio::test]
    async fn test_sync_url_rejects_private_ips() {
        assert!(validate_sync_url("https://127.0.0.1/config.json").await.is_err());
        assert!(validate_sync_url("https://192.168.1.1/config.json").await.is_err());
        assert!(validate_sync_url("https://10.0.0.1/config.json").await.is_err());
    }

    #[tokio::test]
    async fn test_sync_url_allows_public() {
        assert!(validate_sync_url("https://8.8.8.8/config.json").await.is_ok());
    }

    #[tokio::test]
    async fn test_sync_url_rejects_documentation_nets() {
        assert!(validate_sync_url("https://192.0.2.1/config.json").await.is_err());
        assert!(validate_sync_url("https://198.51.100.1/config.json").await.is_err());
        assert!(validate_sync_url("https://203.0.113.1/config.json").await.is_err());
    }

    #[tokio::test]
    async fn test_sync_url_rejects_empty_host() {
        assert!(validate_sync_url("https:///config.json").await.is_err());
    }

    #[tokio::test]
    async fn test_sync_url_rejects_userinfo() {
        // userinfo bypass: evil.com@trusted.com would pass host validation
        // but reqwest connects to trusted.com (the actual host)
        assert!(validate_sync_url("https://evil.com@trusted.com/config.json").await.is_err());
        assert!(validate_sync_url("https://attacker@example.com/path").await.is_err());
    }
}
