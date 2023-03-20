use {axum::response::IntoResponse, hyper::StatusCode, tracing::error};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Envy(#[from] envy::Error),

    #[error(transparent)]
    RpcAuth(#[from] walletconnect_sdk::rpc::auth::Error),

    #[error(transparent)]
    Trace(#[from] opentelemetry::trace::TraceError),

    #[error(transparent)]
    Metrics(#[from] opentelemetry::metrics::MetricsError),

    #[error(transparent)]
    Database(#[from] mongodb::error::Error),

    #[error(transparent)]
    Url(#[from] url::ParseError),

    #[error(transparent)]
    Hex(#[from] hex::FromHexError),

    #[error(transparent)]
    Prometheus(#[from] prometheus_core::Error),

    #[error(transparent)]
    SerdeJson(#[from] serde_json::error::Error),

    #[error(transparent)]
    WebSocket(#[from] tungstenite::Error),

    #[error("Tried to interact with channel that's already closed")]
    ChannelClosed,

    #[error("Failed to receive on websocket")]
    RecvError,

    #[error("Failed to parse the keypair seed")]
    InvalidKeypairSeed,
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        error!("{:?}", self);
        match self {
            Self::Database(_) => (
                StatusCode::BAD_REQUEST,
                "Client seems to already be registered for this project id",
            )
                .into_response(),
            Self::Url(_) => (StatusCode::BAD_REQUEST, "Invalid url. ").into_response(),
            Self::SerdeJson(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "Serialization failure.").into_response()
            }
            Self::Hex(_) => (StatusCode::BAD_REQUEST, "Invalid symmetric key").into_response(),
            _ => (StatusCode::NOT_FOUND, "Not found.").into_response(),
        }
    }
}
