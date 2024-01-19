use {
    crate::{
        auth::JwtError, error::NotifyServerError, rate_limit::RateLimitExceeded,
        types::EnvelopeParseError,
    },
    relay_rpc::domain::Topic,
};

#[derive(Debug, thiserror::Error)]
pub enum RelayMessageClientError {
    #[error("Received wc_notifyWatchSubscriptions (4010) on wrong topic: {0}")]
    WrongNotifyWatchSubscriptionsTopic(Topic),

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
}

#[derive(Debug, thiserror::Error)]
pub enum RelayMessageServerError {
    #[error(transparent)]
    NotifyServerError(#[from] NotifyServerError),
}

#[derive(Debug, thiserror::Error)]
pub enum RelayMessageError {
    #[error("Relay message client error: {0}")]
    Client(#[from] RelayMessageClientError),

    #[error("Relay message server error: {0}")]
    Server(#[from] RelayMessageServerError),
}
