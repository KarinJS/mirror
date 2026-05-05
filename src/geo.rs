use crate::config::{GeoConfig, GeoMode};

pub fn check_geo(config: &GeoConfig, country: Option<&str>) -> bool {
    match config.mode {
        GeoMode::Off => true,
        GeoMode::Allow => {
            if let Some(c) = country {
                config.countries.iter().any(|allowed| allowed == c)
            } else {
                false // Fail closed
            }
        }
        GeoMode::Deny => {
            if let Some(c) = country {
                !config.countries.iter().any(|denied| denied == c)
            } else {
                false // Fail closed
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_geo_off() {
        let config = GeoConfig {
            mode: GeoMode::Off,
            header_name: "EO-Client-Country".to_string(),
            countries: vec!["CN".to_string()],
        };

        assert!(check_geo(&config, Some("CN")));
        assert!(check_geo(&config, Some("US")));
        assert!(check_geo(&config, None));
    }

    #[test]
    fn test_check_geo_allow() {
        let config = GeoConfig {
            mode: GeoMode::Allow,
            header_name: "EO-Client-Country".to_string(),
            countries: vec!["CN".to_string(), "HK".to_string()],
        };

        assert!(check_geo(&config, Some("CN")));
        assert!(check_geo(&config, Some("HK")));
        assert!(!check_geo(&config, Some("US")));
        assert!(!check_geo(&config, None)); // Fail closed
    }

    #[test]
    fn test_check_geo_deny() {
        let config = GeoConfig {
            mode: GeoMode::Deny,
            header_name: "EO-Client-Country".to_string(),
            countries: vec!["US".to_string(), "UK".to_string()],
        };

        assert!(!check_geo(&config, Some("US")));
        assert!(!check_geo(&config, Some("UK")));
        assert!(check_geo(&config, Some("CN")));
        assert!(!check_geo(&config, None)); // Fail closed
    }
}
