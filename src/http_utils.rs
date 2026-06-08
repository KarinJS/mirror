use axum::http::HeaderMap;

pub fn get_client_country(headers: &HeaderMap, header_name: &str) -> Option<String> {
    headers
        .get(header_name)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| {
            // Validate country code: must be exactly 2 ASCII uppercase letters (ISO 3166-1 alpha-2)
            s.len() == 2 && s.bytes().all(|b| b.is_ascii_uppercase())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;

    #[test]
    fn test_get_client_country() {
        let mut headers = HeaderMap::new();
        headers.insert("EO-Client-Country", "CN".parse().unwrap());

        let country = get_client_country(&headers, "EO-Client-Country");
        assert_eq!(country, Some("CN".to_string()));

        let country = get_client_country(&headers, "X-Country");
        assert_eq!(country, None);

        let empty_headers = HeaderMap::new();
        let country = get_client_country(&empty_headers, "EO-Client-Country");
        assert_eq!(country, None);
    }

    #[test]
    fn test_country_code_validation() {
        let mut headers = HeaderMap::new();

        // Valid 2-letter uppercase codes
        headers.insert("X-Country", "CN".parse().unwrap());
        assert_eq!(get_client_country(&headers, "X-Country"), Some("CN".to_string()));

        headers.insert("X-Country", "US".parse().unwrap());
        assert_eq!(get_client_country(&headers, "X-Country"), Some("US".to_string()));

        // Reject lowercase
        headers.insert("X-Country", "cn".parse().unwrap());
        assert_eq!(get_client_country(&headers, "X-Country"), None);

        // Reject mixed case
        headers.insert("X-Country", "Cn".parse().unwrap());
        assert_eq!(get_client_country(&headers, "X-Country"), None);

        // Reject too long
        headers.insert("X-Country", "CHN".parse().unwrap());
        assert_eq!(get_client_country(&headers, "X-Country"), None);

        // Reject too short
        headers.insert("X-Country", "C".parse().unwrap());
        assert_eq!(get_client_country(&headers, "X-Country"), None);

        // Reject injection attempts
        headers.insert("X-Country", "CN; DROP TABLE".parse().unwrap());
        assert_eq!(get_client_country(&headers, "X-Country"), None);

        // Reject empty
        headers.insert("X-Country", "".parse().unwrap());
        assert_eq!(get_client_country(&headers, "X-Country"), None);
    }
}
