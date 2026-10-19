use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

pub struct ApiError(pub StatusCode, pub String);

pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    pub fn new(code: StatusCode, msg: impl Into<String>) -> Self {
        Self(code, msg.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::RowNotFound => Self(StatusCode::NOT_FOUND, "not found".into()),
            e => {
                tracing::error!("db: {e}");
                Self(StatusCode::INTERNAL_SERVER_ERROR, "database error".into())
            }
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        tracing::error!("{e:#}");
        Self(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    }
}
