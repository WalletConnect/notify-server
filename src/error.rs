use {
    crate::{
        analytics::AnalyticsInitError, auth, model::types::AccountId,
        services::websocket_server::handlers::notify_watch_subscriptions::CheckAppAuthorizationError,
    },
    axum::{response::IntoResponse, Json},
    data_encoding::DecodeError,
    hyper::StatusCode,
    relay_rpc::domain::{ClientIdDecodingError, ProjectId, Topic},
    serde_json::json,
    std::string::FromUtf8Error,
    tracing::{error, warn},
};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Invalid event for webhook")]
    InvalidEvent,

    #[error("The provided url has invalid scheme")]
    InvalidScheme,

    #[error("The Relay client has stopped working")]
    RelayClientStopped,

    #[error("Invalid account")]
    InvalidAccount,

    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),

    #[error(transparent)]
    Envy(#[from] envy::Error),

    #[error(transparent)]
    RpcAuth(#[from] relay_rpc::auth::Error),

    #[error(transparent)]
    Url(#[from] url::ParseError),

    #[error("URL missing host")]
    UrlMissingHost,

    #[error(transparent)]
    Hex(#[from] hex::FromHexError),

    #[error(transparent)]
    Uuid(#[from] uuid::Error),

    #[error(transparent)]
    Prometheus(#[from] prometheus_core::Error),

    #[error(transparent)]
    SerdeJson(#[from] serde_json::error::Error),

    #[error(transparent)]
    WebSocket(#[from] tungstenite::Error),

    #[error(transparent)]
    Broadcast(#[from] tokio::sync::broadcast::error::TryRecvError),

    #[error(transparent)]
    FromUtf8Error(#[from] FromUtf8Error),

    #[error(transparent)]
    TokioTimeElapsed(#[from] tokio::time::error::Elapsed),

    #[error(transparent)]
    Base64Decode(#[from] base64::DecodeError),

    #[error(transparent)]
    WalletConnectClient(#[from] relay_client::error::Error),

    #[error(transparent)]
    TryRecvError(#[from] tokio::sync::mpsc::error::TryRecvError),

    #[error("No project found associated with topic {0}")]
    NoProjectDataForTopic(Topic),

    #[error("No project found associated with app_domain {0}")]
    NoProjectDataForAppDomain(String),

    #[error("No client found associated with topic {0}")]
    NoClientDataForTopic(Topic),

    #[error("No project found associated with project ID {0}")]
    NoProjectDataForProjectId(ProjectId),

    #[error("No client found associated with project ID {0} and account {1}")]
    NoClientDataForProjectIdAndAccount(ProjectId, AccountId),

    #[error("Tried to interact with channel that's already closed")]
    ChannelClosed,

    #[error(transparent)]
    DecodeError(#[from] DecodeError),

    #[error("Missing {0}")]
    SubscriptionAuthError(String),

    #[error(transparent)]
    TryFromSliceError(#[from] std::array::TryFromSliceError),

    #[error("Invalid key length")]
    HkdfInvalidLength,

    #[error("Failed to get value from json")]
    JsonGetError,

    #[error("Cryptography failure: {0}")]
    EncryptionError(String),

    #[error("Failed to receive on websocket")]
    RecvError,

    #[error(transparent)]
    SystemTimeError(#[from] std::time::SystemTimeError),

    #[error("Failed to parse the keypair seed")]
    InvalidKeypairSeed,

    #[error("GeoIpReader Error: {0}")]
    GeoIpReader(String),

    #[error("BatchCollector Error: {0}")]
    BatchCollector(String),

    #[error(transparent)]
    JwtVerificationError(#[from] auth::AuthError),

    #[error(transparent)]
    ClientIdDecodingError(#[from] ClientIdDecodingError),

    #[error(transparent)]
    ChronoParse(#[from] chrono::ParseError),

    #[error(transparent)]
    AnalyticsInitError(#[from] AnalyticsInitError),

    #[error(transparent)]
    Redis(#[from] crate::registry::storage::error::StorageError),

    #[error(transparent)]
    InvalidHeaderValue(#[from] hyper::header::InvalidHeaderValue),

    #[error(transparent)]
    ToStrError(#[from] hyper::header::ToStrError),

    #[error(transparent)]
    EdDalek(#[from] ed25519_dalek::ed25519::Error),

    #[error("Received wc_notifyWatchSubscriptions on wrong topic: {0}")]
    WrongNotifyWatchSubscriptionsTopic(Topic),

    #[error("Not authorized to control that app's subscriptions")]
    AppSubscriptionsUnauthorized,

    #[error("The requested app does not match the project's app domain")]
    AppDoesNotMatch,

    #[error("`app` invalid, not a did:web")]
    AppNotDidWeb,

    #[error(transparent)]
    AppNotAuthorized(#[from] CheckAppAuthorizationError),

    #[error("sqlx error: {0}")]
    SqlxError(#[from] sqlx::error::Error),

    #[error("sqlx migration error: {0}")]
    SqlxMigrationError(#[from] sqlx::migrate::MigrateError),

    #[error("Failed to set scheme")]
    UrlSetScheme,

    #[error("Bad request: {0}")]
    BadRequest(String),
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        warn!("Error response: {:?}", self);
        match self {
            Self::Url(_) => (StatusCode::BAD_REQUEST, "Invalid url. ").into_response(),
            Self::Hex(_) => (StatusCode::BAD_REQUEST, "Invalid symmetric key").into_response(),
            Self::BadRequest(e) => (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": e
                })),
            )
                .into_response(),
            error => {
                error!("Unhandled error: {:?}", error);
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error.").into_response()
            }
        }
    }
}
