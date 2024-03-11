use {
    crate::{
        auth::{
            IdentityVerificationClientError, IdentityVerificationError,
            IdentityVerificationInternalError, JwtError,
        },
        error::NotifyServerError,
        rate_limit::RateLimitExceeded,
        rpc::JsonRpcError,
        types::EnvelopeParseError,
    },
    relay_rpc::domain::Topic,
    std::sync::Arc,
};

#[derive(Debug, thiserror::Error)]
pub enum RelayMessageClientError {
    #[error("No project found associated with topic {0}")]
    WrongNotifySubscribeTopic(Topic),

    #[error("Received 4010 on wrong topic: {0}")]
    WrongNotifyWatchSubscriptionsTopic(Topic),

    #[error("Subscription watcher limit reached")]
    SubscriptionWatcherLimitReached,

    #[error("Received 4008 on unrecognized topic: {0}")]
    WrongNotifyUpdateTopic(Topic),

    #[error("Received 4004 on unrecognized topic: {0}")]
    WrongNotifyDeleteTopic(Topic),

    #[error("Received 4014 on unrecognized topic: {0}")]
    WrongNotifyGetNotificationsTopic(Topic),

    #[error("No project found associated with app_domain {0}")]
    NotifyWatchSubscriptionsAppDomainNotFound(Arc<str>),

    #[error("The requested app does not match the project's app domain")]
    AppDoesNotMatch,

    #[error("Decode message: {0}")]
    DecodeMessage(#[from] base64::DecodeError),

    #[error("Could not parse message envelope: {0}")]
    EnvelopeParseError(#[from] EnvelopeParseError),

    #[error(transparent)]
    RateLimitExceeded(RateLimitExceeded),

    #[error("Not authorized to control that app's subscriptions")]
    AppSubscriptionsUnauthorized,

    #[error("JWT parse/verification error: {0}")]
    JwtError(JwtError),

    #[error(transparent)]
    IdentityVerification(IdentityVerificationClientError),
}

#[derive(Debug, thiserror::Error)]
pub enum RelayMessageServerError {
    #[error(transparent)]
    NotifyServerError(#[from] NotifyServerError),

    #[error(transparent)]
    IdentityVerification(IdentityVerificationInternalError),
}

#[derive(Debug, thiserror::Error)]
pub enum RelayMessageError {
    #[error("Relay message client error: {0}")]
    Client(#[from] RelayMessageClientError),

    #[error("Relay message server error: {0}")]
    Server(#[from] RelayMessageServerError),
}

impl From<IdentityVerificationError> for RelayMessageError {
    fn from(err: IdentityVerificationError) -> Self {
        match err {
            IdentityVerificationError::Client(err) => {
                RelayMessageError::Client(RelayMessageClientError::IdentityVerification(err))
            }
            IdentityVerificationError::Internal(err) => {
                RelayMessageError::Server(RelayMessageServerError::IdentityVerification(err))
            }
        }
    }
}

impl From<RelayMessageError> for JsonRpcError {
    fn from(err: RelayMessageError) -> Self {
        match err {
            RelayMessageError::Client(err) => JsonRpcError {
                message: err.to_string(),
                code: -1, // TODO more detailed codes
            },
            RelayMessageError::Server(_err) => JsonRpcError {
                message: "Internal server error".to_owned(),
                code: -32000,
            },
        }
    }
}
