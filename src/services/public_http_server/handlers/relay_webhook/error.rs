use {
    super::handlers::notify_watch_subscriptions::{
        CheckAppAuthorizationError, CollectSubscriptionsError, PrepareSubscriptionWatchersError,
        SubscriptionWatcherSendError,
    },
    crate::{
        auth::{
            IdentityVerificationClientError, IdentityVerificationError,
            IdentityVerificationInternalError, JwtError, SignJwtError,
        },
        error::NotifyServerError,
        model::types::GetAuthenticationClientIdError,
        rate_limit::RateLimitExceeded,
        rpc::{DecodeKeyError, DeriveKeyError, JsonRpcError},
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

    #[error("Received 4016 on unrecognized topic: {0}")]
    WrongNotifyMarkNotificationsAsReadTopic(Topic),

    #[error("No project found associated with app_domain {0}")]
    NotifyWatchSubscriptionsAppDomainNotFound(Arc<str>),

    #[error("The requested app does not match the project's app domain")]
    AppDoesNotMatch,

    #[error("Decode message: {0}")]
    DecodeMessage(base64::DecodeError),

    #[error("Could not parse message envelope: {0}")]
    EnvelopeParse(EnvelopeParseError),

    #[error("Decryption error: {0}")]
    Decryption(chacha20poly1305::aead::Error),

    #[error("JSON deserialization error: {0}")]
    JsonDeserialization(serde_json::Error),

    #[error(transparent)]
    RateLimitExceeded(RateLimitExceeded),

    #[error("Not authorized to control that app's subscriptions")]
    AppSubscriptionsUnauthorized,

    #[error("JWT parse/verification error: {0}")]
    Jwt(JwtError),

    #[error("Identity key verification: {0}")]
    IdentityVerification(IdentityVerificationClientError),

    #[error(transparent)]
    AppNotAuthorized(#[from] CheckAppAuthorizationError),

    #[error("The account authenticated cannot control this subscription")]
    AccountNotAuthorized,

    #[error("Message received on topic, but the key associated with that topic does not hash to the topic")]
    TopicDoesNotMatchKey,
}

#[derive(Debug, thiserror::Error)]
pub enum RelayMessageServerError {
    #[error("NotifyServerError: {0}")]
    NotifyServer(#[from] NotifyServerError),

    #[error("Identity key verification: {0}")]
    IdentityVerification(IdentityVerificationInternalError),

    #[error("Envelope encryption: {0}")]
    EnvelopeEncryption(chacha20poly1305::aead::Error),

    #[error("Decode key: {0}")]
    DecodeKey(DecodeKeyError),

    #[error("Derive key: {0}")]
    DeriveKey(DeriveKeyError),

    #[error("Get authentication client id: {0}")]
    GetAuthenticationClientId(#[from] GetAuthenticationClientIdError),

    #[error("Sign JWT: {0}")]
    SignJwt(#[from] SignJwtError),

    #[error("JSON-RPC response serialization: {0}")]
    JsonRpcResponseSerialization(serde_json::error::Error),

    #[error("JSON-RPC response error serialization: {0}")]
    JsonRpcResponseErrorSerialization(serde_json::error::Error),

    #[error("Collect subscription: {0}")]
    CollectSubscriptions(CollectSubscriptionsError),

    #[error("Prepare subscription watchers: {0}")]
    PrepareSubscriptionWatchers(PrepareSubscriptionWatchersError),

    #[error("Subscription watcher send: {0}")]
    SubscriptionWatcherSend(SubscriptionWatcherSendError),

    #[error("Error sending sdk info via oneshot channel")]
    SdkOneshotSend,
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

impl From<&RelayMessageError> for JsonRpcError {
    fn from(err: &RelayMessageError) -> Self {
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
