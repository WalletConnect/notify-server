use {
    self::error::RelayMessageServerError,
    crate::{
        metrics::RelayIncomingMessageStatus,
        services::public_http_server::handlers::relay_webhook::{
            error::RelayMessageError,
            handlers::{
                notify_delete, notify_get_notifications, notify_subscribe, notify_update,
                notify_watch_subscriptions,
            },
        },
        spec,
        state::AppState,
    },
    axum::{
        extract::State,
        http::StatusCode,
        response::{IntoResponse, Response},
        Json,
    },
    relay_rpc::{
        domain::Topic,
        jwt::{JwtError, VerifyableClaims},
        rpc::{
            msg_id::get_message_id, WatchAction, WatchEventClaims, WatchStatus, WatchType,
            WatchWebhookPayload,
        },
    },
    serde_json::json,
    std::{collections::HashSet, sync::Arc, time::Instant},
    thiserror::Error,
    tracing::{error, info, instrument, warn},
};

pub mod error;
pub mod handlers;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("Could not parse watch event claims: {0}")]
    ParseWatchEvent(JwtError),

    #[error("Could not verify watch event: {0}")]
    VerifyWatchEvent(JwtError),

    #[error("JWT has wrong issuer")]
    WrongIssuer,

    #[error("Expected WatchAction::WatchEvent, got {0:?}")]
    WrongWatchAction(WatchAction),

    #[error("Expected WatchType::Subscriber, got {0:?}")]
    WrongWatchType(WatchType),

    #[error("Expected WatchStatus::Queued, got {0:?}")]
    WrongWatchStatus(WatchStatus),
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("Client error: {0}")]
    ClientError(ClientError),

    #[error("Server error: {0}")]
    ServerError(RelayMessageServerError),
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        match self {
            Error::ClientError(e) => {
                warn!("Relay webhook client error: {e:?}");
                (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(json!({ "error": e.to_string() })),
                )
                    .into_response()
            }
            Error::ServerError(e) => {
                error!("Relay webhook server error: {e:?}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": "Internal server error" })),
                )
                    .into_response()
            }
        }
    }
}

pub async fn handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<WatchWebhookPayload>,
) -> Result<Response, Error> {
    let claims = WatchEventClaims::try_from_str(&payload.event_auth)
        .map_err(|e| Error::ClientError(ClientError::ParseWatchEvent(e)))?;

    claims
        .verify_basic(&HashSet::from([state.config.notify_url.to_string()]), None)
        .map_err(|e| Error::ClientError(ClientError::VerifyWatchEvent(e)))?;

    if claims.basic.iss != state.relay_identity {
        return Err(Error::ClientError(ClientError::WrongIssuer));
    }

    // TODO check sub

    // TODO irn_batchReceive message

    if claims.act != WatchAction::WatchEvent {
        return Err(Error::ClientError(ClientError::WrongWatchAction(
            claims.act,
        )));
    }
    if claims.typ != WatchType::Subscriber {
        return Err(Error::ClientError(ClientError::WrongWatchType(claims.typ)));
    }
    // TODO check whu

    let event = claims.evt;
    if event.status != WatchStatus::Queued {
        return Err(Error::ClientError(ClientError::WrongWatchStatus(
            event.status,
        )));
    }

    let msg = RelayIncomingMessage {
        topic: event.topic,
        message: event.message,
        tag: event.tag,
    };
    handle_msg(msg, &state).await.map_err(Error::ServerError)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

pub struct RelayIncomingMessage {
    pub topic: Topic,
    pub message: Arc<str>,
    pub tag: u32,
}

#[instrument(skip_all, fields(topic = %msg.topic, tag = %msg.tag, message_id = %get_message_id(&msg.message)))]
async fn handle_msg(
    msg: RelayIncomingMessage,
    state: &AppState,
) -> Result<(), RelayMessageServerError> {
    let start = Instant::now();
    let topic = msg.topic.clone();
    let tag = msg.tag;
    info!("Received tag {tag} on topic {topic}");

    let result = match tag {
        spec::NOTIFY_SUBSCRIBE_TAG => notify_subscribe::handle(msg, state).await,
        spec::NOTIFY_DELETE_TAG => notify_delete::handle(msg, state).await,
        spec::NOTIFY_UPDATE_TAG => notify_update::handle(msg, state).await,
        spec::NOTIFY_WATCH_SUBSCRIPTIONS_TAG => {
            notify_watch_subscriptions::handle(msg, state).await
        }
        spec::NOTIFY_GET_NOTIFICATIONS_TAG => notify_get_notifications::handle(msg, state).await,
        _ => {
            warn!("Ignored tag {tag} on topic {topic}");
            Ok(())
        }
    };

    let (status, result) = if let Err(e) = result {
        match e {
            RelayMessageError::Client(e) => {
                warn!("Relay message client error handling {tag} on topic {topic}: {e}");
                (RelayIncomingMessageStatus::ClientError, Ok(()))
            }
            RelayMessageError::Server(e) => {
                error!("Relay message server error handling {tag} on topic {topic}: {e}");
                (RelayIncomingMessageStatus::ServerError, Err(e))
            }
        }
    } else {
        info!("Success processing {tag} on topic {topic}");
        (RelayIncomingMessageStatus::Success, Ok(()))
    };

    if let Some(metrics) = &state.metrics {
        metrics.relay_incoming_message(tag, status, start);
    }

    result
}
