use axum::http::HeaderMap;

pub fn get_client_country(headers: &HeaderMap, header_name: &str) -> Option<String> {
    headers
        .get(header_name)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
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
}
