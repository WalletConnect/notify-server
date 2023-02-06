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
    Store(#[from] crate::stores::StoreError),

    #[error(transparent)]
    Insert(#[from] mongodb::error::Error),

    #[error("Failed to parse the keypair seed")]
    InvalidKeypairSeed,
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        match self {
            err @ Self::Insert(_) => {
                dbg!(err);
                (StatusCode::NOT_MODIFIED, "Record already exists").into_response()
            }
            _ => (StatusCode::NOT_FOUND, "Not found.").into_response(),
        }
    }
}
