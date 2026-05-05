use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use std::fmt;

#[derive(Debug)]
pub enum AppError {
    NotFound,
    GeoBlocked,
    Unauthorized,
    PayloadTooLarge,
    BadGateway,
    GatewayTimeout,
}

pub type AppResult<T> = Result<T, AppError>;

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::NotFound => write!(f, "not found"),
            AppError::GeoBlocked => write!(f, "geo blocked"),
            AppError::Unauthorized => write!(f, "unauthorized"),
            AppError::PayloadTooLarge => write!(f, "payload too large"),
            AppError::BadGateway => write!(f, "bad gateway"),
            AppError::GatewayTimeout => write!(f, "gateway timeout"),
        }
    }
}

impl std::error::Error for AppError {}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match self {
            AppError::NotFound => StatusCode::NOT_FOUND,
            AppError::GeoBlocked => StatusCode::FORBIDDEN,
            AppError::Unauthorized => StatusCode::UNAUTHORIZED,
            AppError::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            AppError::BadGateway => StatusCode::BAD_GATEWAY,
            AppError::GatewayTimeout => StatusCode::GATEWAY_TIMEOUT,
        };
        status.into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_error_into_response() {
        let err = AppError::NotFound;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let err = AppError::GeoBlocked;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let err = AppError::PayloadTooLarge;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);

        let err = AppError::BadGateway;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);

        let err = AppError::GatewayTimeout;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
    }
}
