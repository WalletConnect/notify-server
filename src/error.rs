// use axum_core::response::IntoResponse;
use {
    crate::{
        analytics::AnalyticsInitError,
        auth,
        rate_limit::{InternalRateLimitError, RateLimitExceeded},
        services::public_http_server::handlers::notification_link::GetGeoError,
    },
    axum::{response::IntoResponse, Json},
    hyper::StatusCode,
    relay_client::error::ClientError,
    relay_rpc::{
        auth::ed25519_dalek::ed25519,
        rpc::{PublishError, SubscriptionError, WatchError},
    },
    serde_json::json,
    thiserror::Error,
    tracing::{error, info, warn},
};

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
    RelayClient(#[from] ClientError),

    #[error(transparent)]
    RelaySubscribe(#[from] relay_client::error::Error<SubscriptionError>),

    #[error(transparent)]
    RelayPublish(#[from] relay_client::error::Error<PublishError>),

    #[error(transparent)]
    RelayWatch(#[from] relay_client::error::Error<WatchError>),

    #[error("Failed to parse the keypair seed")]
    InvalidKeypairSeed,

    #[error(transparent)]
    JwtVerification(#[from] auth::AuthError),

    #[error(transparent)]
    AnalyticsInit(#[from] AnalyticsInitError),

    #[error(transparent)]
    RegistryStorage(#[from] crate::registry::storage::error::StorageError),

    #[error(transparent)]
    InvalidHeaderValue(#[from] hyper::header::InvalidHeaderValue),

    #[error(transparent)]
    EdDalek(#[from] ed25519::Error),

    #[error("sqlx error: {0}")]
    Sqlx(#[from] sqlx::error::Error),

    #[error("sqlx migration error: {0}")]
    SqlxMigration(#[from] sqlx::migrate::MigrateError),

    #[error("Failed to set scheme")]
    UrlSetScheme,

    #[error("Unprocessable entity: {0}")]
    UnprocessableEntity(String),

    #[error("App domain in-use by another project")]
    AppDomainInUseByAnotherProject,

    #[error("Redis pool error: {0}")]
    RedisPool(#[from] deadpool_redis::PoolError),

    #[error("Redis error: {0}")]
    Redis(#[from] redis::RedisError),

    #[error(transparent)]
    TooManyRequests(RateLimitExceeded),

    #[error("Rate limit error: {0}")]
    RateLimit(#[from] InternalRateLimitError),

    #[error("Get geo: {0}")]
    GetGeo(#[from] GetGeoError),
}

impl IntoResponse for NotifyServerError {
    fn into_response(self) -> axum::response::Response {
        info!("Responding with error: {self:?}");
        let response = match &self {
            Self::Url(_) => (StatusCode::BAD_REQUEST, "Invalid url. ").into_response(),
            Self::Hex(_) => (StatusCode::BAD_REQUEST, "Invalid symmetric key").into_response(),
            Self::UnprocessableEntity(e) => (
                StatusCode::UNPROCESSABLE_ENTITY,
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
