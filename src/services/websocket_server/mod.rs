use {
    crate::{
        metrics::{Metrics, RelayIncomingMessageStatus},
        model::helpers::{get_project_topics, get_subscriber_topics},
        relay_client_helpers::create_ws_connect_options,
        services::websocket_server::handlers::{
            notify_delete, notify_get_notifications, notify_subscribe, notify_update,
            notify_watch_subscriptions,
        },
        spec::{
            NOTIFY_DELETE_TAG, NOTIFY_GET_NOTIFICATIONS_TAG, NOTIFY_SUBSCRIBE_TAG,
            NOTIFY_UPDATE_TAG, NOTIFY_WATCH_SUBSCRIPTIONS_TAG,
        },
        state::AppState,
        Result,
    },
    futures_util::{StreamExt, TryFutureExt, TryStreamExt},
    rand::Rng,
    relay_client::websocket::{Client, PublishedMessage},
    relay_rpc::{
        domain::{MessageId, Topic},
        rpc::{msg_id::get_message_id, JSON_RPC_VERSION_STR, MAX_SUBSCRIPTION_BATCH_SIZE},
    },
    relay_ws_client::RelayClientEvent,
    serde::{Deserialize, Serialize},
    sha2::Sha256,
    sqlx::PgPool,
    std::{convert::Infallible, sync::Arc, time::Instant},
    tokio::sync::mpsc::UnboundedReceiver,
    tracing::{error, info, instrument, warn},
    wc::metrics::otel::Context,
};

pub mod handlers;
pub mod relay_ws_client;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestBody {
    pub id: MessageId,
    pub jsonrpc: String,
    pub params: String,
}

async fn connect(state: &AppState, client: &Client) -> Result<()> {
    info!("Connecting to relay");
    client
        .connect(&create_ws_connect_options(
            &state.keypair,
            state.config.relay_url.clone(),
            state.config.notify_url.clone(),
            state.config.project_id.clone(),
        )?)
        .await?;

    resubscribe(
        state.notify_keys.key_agreement_topic.clone(),
        &state.postgres,
        client,
        state.metrics.as_ref(),
    )
    .await?;

    Ok(())
}

pub async fn start(
    state: Arc<AppState>,
    relay_ws_client: Arc<Client>,
    mut rx: UnboundedReceiver<RelayClientEvent>,
) -> Result<Infallible> {
    connect(&state, &relay_ws_client).await?;
    loop {
        let Some(msg) = rx.recv().await else {
            return Err(crate::error::Error::RelayClientStopped);
        };
        match msg {
            relay_ws_client::RelayClientEvent::Message(msg) => {
                let state = state.clone();
                let relay_ws_client = relay_ws_client.clone();
                tokio::spawn(async move { handle_msg(msg, &state, &relay_ws_client).await });
            }
            relay_ws_client::RelayClientEvent::Error(e) => {
                warn!("Received error from relay: {e}");
                while let Err(e) = connect(&state, &relay_ws_client).await {
                    error!("Error reconnecting to relay: {}", e);
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
            relay_ws_client::RelayClientEvent::Disconnected(e) => {
                info!("Received disconnect from relay: {e:?}");
                while let Err(e) = connect(&state, &relay_ws_client).await {
                    warn!("Error reconnecting to relay: {e}");
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
            relay_ws_client::RelayClientEvent::Connected => {
                info!("Connected to relay");
            }
        }
    }
}

#[instrument(skip_all, fields(topic = %msg.topic, tag = %msg.tag, message_id = %get_message_id(&msg.message)))]
async fn handle_msg(msg: PublishedMessage, state: &AppState, client: &Client) {
    let start = Instant::now();
    let topic = msg.topic.clone();
    let tag = msg.tag;
    info!("Received tag {tag} on topic {topic}");

    let result = match tag {
        NOTIFY_DELETE_TAG => notify_delete::handle(msg, state, client).await,
        NOTIFY_SUBSCRIBE_TAG => notify_subscribe::handle(msg, state).await,
        NOTIFY_UPDATE_TAG => notify_update::handle(msg, state).await,
        NOTIFY_WATCH_SUBSCRIPTIONS_TAG => notify_watch_subscriptions::handle(msg, state).await,
        NOTIFY_GET_NOTIFICATIONS_TAG => notify_get_notifications::handle(msg, state).await,
        _ => {
            info!("Ignored tag {tag} on topic {topic}");
            Ok(())
        }
    };

    let status = if let Err(e) = result {
        error!("Error handling {tag} on topic {topic}: {e}");
        RelayIncomingMessageStatus::ServerError
    } else {
        info!("Finished processing {tag} on topic {topic}");
        RelayIncomingMessageStatus::Success
    };

    if let Some(metrics) = &state.metrics {
        metrics.relay_incoming_message(tag, status, start);
    }
}

#[instrument(skip_all)]
async fn resubscribe(
    key_agreement_topic: Topic,
    postgres: &PgPool,
    client: &Client,
    metrics: Option<&Metrics>,
) -> Result<()> {
    info!("Resubscribing to all topics");
    let start = Instant::now();

    let subscriber_topics = get_subscriber_topics(postgres, metrics).await?;
    let subscriber_topics_count = subscriber_topics.len();
    info!("subscriber_topics_count: {subscriber_topics_count}");

    let project_topics = get_project_topics(postgres, metrics).await?;
    let project_topics_count = project_topics.len();
    info!("project_topics_count: {project_topics_count}");

    // TODO: These need to be paginated and streamed from the database directly
    // instead of collecting them to a single giant vec.
    let topics = [key_agreement_topic]
        .into_iter()
        .chain(subscriber_topics.into_iter())
        .chain(project_topics.into_iter())
        .collect::<Vec<_>>();
    let topics_count = topics.len();
    info!("topics_count: {topics_count}");

    // Collect each batch into its own vec, since `batch_subscribe` would convert
    // them anyway.
    let topics = topics
        .chunks(MAX_SUBSCRIPTION_BATCH_SIZE)
        .map(|chunk| chunk.to_vec())
        .collect::<Vec<_>>();

    // Limit concurrency to avoid overwhelming the relay with requests.
    const REQUEST_CONCURRENCY: usize = 48;

    futures_util::stream::iter(topics)
        .map(|topics| {
            // Map result to an unsized type to avoid allocation when collecting,
            // as we don't care about subscription IDs.
            client.batch_subscribe_blocking(topics).map_ok(|_| ())
        })
        .buffer_unordered(REQUEST_CONCURRENCY)
        .try_collect::<Vec<_>>()
        .await?;

    let elapsed = start.elapsed().as_millis().try_into().unwrap();
    info!("resubscribe took {elapsed}ms");

    if let Some(metrics) = metrics {
        let ctx = Context::current();
        metrics
            .subscribed_project_topics
            .observe(&ctx, project_topics_count as u64, &[]);
        metrics
            .subscribed_subscriber_topics
            .observe(&ctx, subscriber_topics_count as u64, &[]);
        metrics.subscribe_latency.record(&ctx, elapsed, &[]);
    }

    Ok(())
}

pub fn decode_key(key: &str) -> Result<[u8; 32]> {
    Ok(hex::decode(key)?[..32].try_into()?)
}

pub fn derive_key(
    public_key: &x25519_dalek::PublicKey,
    private_key: &x25519_dalek::StaticSecret,
) -> Result<[u8; 32]> {
    let shared_key = private_key.diffie_hellman(public_key);

    let derived_key = hkdf::Hkdf::<Sha256>::new(None, shared_key.as_bytes());

    let mut expanded_key = [0u8; 32];
    derived_key
        .expand(b"", &mut expanded_key)
        .map_err(|_| crate::error::Error::HkdfInvalidLength)?;
    Ok(expanded_key)
}

#[derive(Serialize, Deserialize, Debug)]
pub struct NotifyRequest<T> {
    pub id: u64,
    pub jsonrpc: String,
    pub method: String,
    pub params: T,
}

impl<T> NotifyRequest<T> {
    pub fn new(method: &str, params: T) -> Self {
        let id = chrono::Utc::now().timestamp_millis().unsigned_abs();
        let id = id * 1000 + rand::thread_rng().gen_range(100, 1000);

        NotifyRequest {
            id,
            jsonrpc: JSON_RPC_VERSION_STR.to_owned(),
            method: method.to_owned(),
            params,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct NotifyResponse<T> {
    pub id: u64,
    pub jsonrpc: String,
    pub result: T,
}

impl<T> NotifyResponse<T> {
    pub fn new(id: u64, result: T) -> Self {
        NotifyResponse {
            id,
            jsonrpc: JSON_RPC_VERSION_STR.to_owned(),
            result,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ResponseAuth {
    pub response_auth: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NotifyWatchSubscriptions {
    pub watch_subscriptions_auth: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NotifySubscriptionsChanged {
    pub subscriptions_changed_auth: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NotifySubscribe {
    pub subscription_auth: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NotifyUpdate {
    pub update_auth: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NotifyDelete {
    pub delete_auth: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AuthMessage {
    pub auth: String,
}
