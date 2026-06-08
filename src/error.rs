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
    RateLimited,
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    fn status_and_message(&self) -> (StatusCode, &'static str) {
        match self {
            AppError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            AppError::GeoBlocked => (StatusCode::FORBIDDEN, "geo_blocked"),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            AppError::PayloadTooLarge => (StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large"),
            AppError::BadGateway => (StatusCode::BAD_GATEWAY, "bad_gateway"),
            AppError::GatewayTimeout => (StatusCode::GATEWAY_TIMEOUT, "gateway_timeout"),
            AppError::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "rate_limited"),
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (_, msg) = self.status_and_message();
        write!(f, "{msg}")
    }
}

impl std::error::Error for AppError {}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = self.status_and_message();
        let body = serde_json::json!({ "error": message });
        (status, axum::Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_app_error_status_codes() {
        let cases = [
            (AppError::NotFound, StatusCode::NOT_FOUND),
            (AppError::GeoBlocked, StatusCode::FORBIDDEN),
            (AppError::Unauthorized, StatusCode::UNAUTHORIZED),
            (AppError::PayloadTooLarge, StatusCode::PAYLOAD_TOO_LARGE),
            (AppError::BadGateway, StatusCode::BAD_GATEWAY),
            (AppError::GatewayTimeout, StatusCode::GATEWAY_TIMEOUT),
            (AppError::RateLimited, StatusCode::TOO_MANY_REQUESTS),
        ];
        for (err, expected_status) in cases {
            let response = err.into_response();
            assert_eq!(response.status(), expected_status);
        }
    }

    #[tokio::test]
    async fn test_app_error_has_json_body() {
        use axum::body::Body;
        use tower::ServiceExt;
        use axum::http::Request;

        let app = axum::Router::new().route(
            "/test",
            axum::routing::get(|| async { Err::<(), _>(AppError::NotFound) }),
        );

        let response = app
            .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "not_found");
    }
}
