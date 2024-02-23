use {
    crate::{
        analytics::AnalyticsInitError,
        auth,
        rate_limit::{InternalRateLimitError, RateLimitExceeded},
    },
    axum::{response::IntoResponse, Json},
    chacha20poly1305::aead,
    hyper::StatusCode,
    relay_client::error::ClientError,
    relay_rpc::{
        auth::{did::DidError, ed25519_dalek::ed25519},
        domain::Topic,
        rpc::{PublishError, SubscriptionError, WatchError},
    },
    serde_json::json,
    std::{array::TryFromSliceError, sync::Arc},
    thiserror::Error,
    tracing::{error, info, warn},
};

// Import not part of group above because it breaks formatting: https://github.com/rust-lang/rustfmt/issues/4746
use crate::services::public_http_server::handlers::relay_webhook::handlers::notify_watch_subscriptions::CheckAppAuthorizationError;

#[derive(Debug, Error)]
pub enum NotifyServerError {
    #[error("Failed to load .env {0}")]
    DotEnvy(#[from] dotenvy::Error),

    #[error("Failed to load configuration from environment {0}")]
    EnvConfiguration(#[from] envy::Error),

    #[error("Invalid account")]
    InvalidAccount,

    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),

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
    SerdeJson(#[from] serde_json::error::Error),

    #[error(transparent)]
    TokioTimeElapsed(tokio::time::error::Elapsed),

    #[error(transparent)]
    RelayClientError(#[from] ClientError),

    #[error(transparent)]
    RelaySubscribeError(#[from] relay_client::error::Error<SubscriptionError>),

    #[error(transparent)]
    RelayPublishError(#[from] relay_client::error::Error<PublishError>),

    #[error(transparent)]
    RelayWatchError(#[from] relay_client::error::Error<WatchError>),

    #[error("No project found associated with topic {0}")]
    NoProjectDataForTopic(Topic),

    #[error("No project found associated with app_domain {0}")]
    NoProjectDataForAppDomain(Arc<str>),

    // TODO move this error somewhere else more specific
    #[error("TryFromSliceError: {0}")]
    TryFromSliceError(#[from] TryFromSliceError),

    // TODO move this error somewhere else more specific
    #[error("InputTooShortError")]
    InputTooShortError,

    #[error("Invalid key length: {0}")]
    HkdfInvalidLength(hkdf::InvalidLength),

    #[error("Cryptography failure: {0}")]
    EncryptionError(aead::Error),

    #[error("Failed to parse the keypair seed")]
    InvalidKeypairSeed,

    #[error(transparent)]
    JwtVerificationError(#[from] auth::AuthError),

    #[error(transparent)]
    AnalyticsInitError(#[from] AnalyticsInitError),

    #[error(transparent)]
    Redis(#[from] crate::registry::storage::error::StorageError),

    #[error(transparent)]
    InvalidHeaderValue(#[from] hyper::header::InvalidHeaderValue),

    #[error(transparent)]
    EdDalek(#[from] ed25519::Error),

    #[error("The requested app does not match the project's app domain")]
    AppDoesNotMatch,

    #[error("`app` invalid, not a did:web: {0}")]
    AppNotDidWeb(DidError),

    #[error(transparent)]
    AppNotAuthorized(#[from] CheckAppAuthorizationError),

    #[error("The account authenticated cannot control this subscription")]
    AccountNotAuthorized,

    #[error("sqlx error: {0}")]
    SqlxError(#[from] sqlx::error::Error),

    #[error("sqlx migration error: {0}")]
    SqlxMigrationError(#[from] sqlx::migrate::MigrateError),

    #[error("Failed to set scheme")]
    UrlSetScheme,

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("App domain in-use by another project")]
    AppDomainInUseByAnotherProject,

    #[error("Redis pool error: {0}")]
    RedisPoolError(#[from] deadpool_redis::PoolError),

    #[error("Redis error: {0}")]
    RedisError(#[from] redis::RedisError),

    #[error(transparent)]
    TooManyRequests(RateLimitExceeded),

    #[error("Message received on topic, but the key associated with that topic does not hash to the topic")]
    TopicDoesNotMatchKey,

    #[error("Rate limit error: {0}")]
    RateLimitError(#[from] InternalRateLimitError),
}

impl IntoResponse for NotifyServerError {
    fn into_response(self) -> axum::response::Response {
        info!("Responding with error: {self:?}");
        let response = match &self {
            Self::Url(_) => (StatusCode::BAD_REQUEST, "Invalid url. ").into_response(),
            Self::Hex(_) => (StatusCode::BAD_REQUEST, "Invalid symmetric key").into_response(),
            Self::BadRequest(e) => (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": e
                })),
            )
                .into_response(),
            Self::AppDomainInUseByAnotherProject => {
                (StatusCode::CONFLICT, self.to_string()).into_response()
            }
            Self::TooManyRequests(_) => {
                (StatusCode::TOO_MANY_REQUESTS, self.to_string()).into_response()
            }
            error => {
                warn!("Error does not have response clause: {:?}", error);
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error.").into_response()
            }
        };

        if response.status().is_client_error() {
            warn!("HTTP client error: {self:?}");
        }

        if response.status().is_server_error() {
            error!("HTTP server error: {self:?}");
        }

        response
    }
}
