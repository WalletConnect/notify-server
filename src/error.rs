use {axum::response::IntoResponse, hyper::StatusCode};
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Envy(#[from] envy::Error),

    #[error(transparent)]
    Trace(#[from] opentelemetry::trace::TraceError),

    #[error(transparent)]
    Metrics(#[from] opentelemetry::metrics::MetricsError),

    #[error(transparent)]
    Store(#[from] mongodb::error::Error),

    #[error(transparent)]
    UrlParse(#[from] url::ParseError),

    #[error(transparent)]
    SerdeJson(#[from] serde_json::error::Error),

    #[error(transparent)]
    Bincode(#[from] bincode::Error),

    #[error("Failed to parse the keypair seed")]
    InvalidKeypairSeed,
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        match self {
            Self::Store(_) => (
                StatusCode::BAD_REQUEST,
                "Client seems to already be registered for this project id",
            )
                .into_response(),
            Self::UrlParse(_) => (StatusCode::BAD_REQUEST, "Invalid url. ").into_response(),
            Self::SerdeJson(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "Serialization failure.").into_response()
            }

            _ => (StatusCode::NOT_FOUND, "Not found.").into_response(),
        }
    }
}
