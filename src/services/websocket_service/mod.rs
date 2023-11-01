use {
    crate::{
        metrics::Metrics,
        model::helpers::{get_project_topics, get_subscriber_topics},
        relay_client_helpers::create_ws_connect_options,
        services::websocket_service::handlers::{
            notify_delete, notify_subscribe, notify_update, notify_watch_subscriptions,
        },
        spec::{
            NOTIFY_DELETE_TAG, NOTIFY_SUBSCRIBE_TAG, NOTIFY_UPDATE_TAG,
            NOTIFY_WATCH_SUBSCRIPTIONS_TAG,
        },
        state::AppState,
        Result,
    },
    rand::Rng,
    relay_client::websocket::Client,
    relay_rpc::{
        domain::{MessageId, Topic},
        rpc::{JSON_RPC_VERSION_STR, MAX_SUBSCRIPTION_BATCH_SIZE},
    },
    serde::{Deserialize, Serialize},
    sha2::Sha256,
    sqlx::PgPool,
    std::{convert::Infallible, sync::Arc, time::Instant},
    tokio::sync::mpsc::UnboundedReceiver,
    tracing::{error, info, instrument, warn},
    wc::metrics::otel::{Context, KeyValue},
    wsclient::RelayClientEvent,
};

pub mod handlers;
pub mod wsclient;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestBody {
    pub id: MessageId,
    pub jsonrpc: String,
    pub params: String,
}

struct WebsocketService {
    state: Arc<AppState>,
    wsclient: Arc<relay_client::websocket::Client>,
}

async fn connect(websocket_service: &WebsocketService) -> Result<()> {
    info!("Connecting to relay");
    websocket_service
        .wsclient
        .connect(&create_ws_connect_options(
            &websocket_service.state.keypair,
            websocket_service.state.config.relay_url.clone(),
            websocket_service.state.config.notify_url.clone(),
            websocket_service.state.config.project_id.clone(),
        )?)
        .await?;

    resubscribe(
        websocket_service
            .state
            .notify_keys
            .key_agreement_topic
            .clone(),
        &websocket_service.state.postgres,
        &websocket_service.wsclient,
        &websocket_service.state.metrics,
    )
    .await?;

    Ok(())
}

pub async fn start(
    app_state: Arc<AppState>,
    wsclient: Arc<Client>,
    mut rx: UnboundedReceiver<RelayClientEvent>,
) -> Result<Infallible> {
    let websocket_service = WebsocketService {
        state: app_state,
        wsclient,
    };
    connect(&websocket_service).await?;
    loop {
        let Some(msg) = rx.recv().await else {
            return Err(crate::error::Error::RelayClientStopped);
        };
        match msg {
            wsclient::RelayClientEvent::Message(msg) => {
                let state = websocket_service.state.clone();
                let wsclient = websocket_service.wsclient.clone();
                tokio::spawn(async move { handle_msg(msg, &state, &wsclient).await });
            }
            wsclient::RelayClientEvent::Error(e) => {
                warn!("Received error from relay: {e}");
                while let Err(e) = connect(&websocket_service).await {
                    error!("Error reconnecting to relay: {}", e);
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
            wsclient::RelayClientEvent::Disconnected(e) => {
                info!("Received disconnect from relay: {e:?}");
                while let Err(e) = connect(&websocket_service).await {
                    warn!("Error reconnecting to relay: {e}");
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
            wsclient::RelayClientEvent::Connected => {
                info!("Connected to relay");
            }
        }
    }
}

#[instrument(skip_all, fields(topic = %msg.topic, tag = %msg.tag, message_id = %sha256::digest(msg.message.as_bytes())))]
async fn handle_msg(
    msg: relay_client::websocket::PublishedMessage,
    state: &Arc<AppState>,
    client: &Arc<relay_client::websocket::Client>,
) {
    let topic = msg.topic.clone();
    let tag = msg.tag;

    info!("Received message");

    match tag {
        NOTIFY_DELETE_TAG => {
            info!("Received notify delete on topic {topic}");
            if let Err(e) = notify_delete::handle(msg, state, client).await {
                warn!("Error handling notify delete: {e}");
            }
            info!("Finished processing notify delete on topic {topic}");
        }
        NOTIFY_SUBSCRIBE_TAG => {
            info!("Received notify subscribe on topic {topic}");
            if let Err(e) = notify_subscribe::handle(msg, state).await {
                warn!("Error handling notify subscribe: {e}");
            }
            info!("Finished processing notify subscribe on topic {topic}");
        }
        NOTIFY_UPDATE_TAG => {
            info!("Received notify update on topic {topic}");
            if let Err(e) = notify_update::handle(msg, state).await {
                warn!("Error handling notify update: {e}");
            }
            info!("Finished processing notify update on topic {topic}");
        }
        NOTIFY_WATCH_SUBSCRIPTIONS_TAG => {
            info!("Received notify watch subscriptions on topic {topic}");
            if let Err(e) = notify_watch_subscriptions::handle(msg, state).await {
                warn!("Error handling notify watch subscriptions: {e}");
            }
            info!("Finished processing notify watch subscriptions on topic {topic}");
        }
        _ => {
            info!("Ignored tag {tag} on topic {topic}");
        }
    }
}

#[instrument(skip_all)]
async fn resubscribe(
    key_agreement_topic: Topic,
    postgres: &PgPool,
    client: &Arc<relay_client::websocket::Client>,
    metrics: &Option<Metrics>,
) -> Result<()> {
    info!("Resubscribing to all topics");
    let start = Instant::now();

    let subscriber_topics = get_subscriber_topics(postgres).await?;
    let subscriber_topics_count = subscriber_topics.len();
    info!("subscriber_topics_count: {subscriber_topics_count}");

    let project_topics = get_project_topics(postgres).await?;
    let project_topics_count = project_topics.len();
    info!("project_topics_count: {project_topics_count}");

    let topics = [key_agreement_topic]
        .into_iter()
        .chain(subscriber_topics.into_iter())
        .chain(project_topics.into_iter())
        .collect::<Vec<_>>();
    let topics_count = topics.len();
    info!("topics_count: {topics_count}");

    let chunks = topics.chunks(MAX_SUBSCRIPTION_BATCH_SIZE);
    for chunk in chunks {
        client.batch_subscribe(chunk).await?;
    }

    let elapsed = start.elapsed().as_millis().try_into().unwrap();
    info!("resubscribe took {elapsed}ms");

    if let Some(metrics) = metrics {
        let ctx = Context::current();
        metrics.subscribed_topics.observe(
            &ctx,
            topics_count as u64,
            &[KeyValue::new("kind", "total")],
        );
        metrics.subscribed_topics.observe(
            &ctx,
            project_topics_count as u64,
            &[KeyValue::new("kind", "project")],
        );
        metrics.subscribed_topics.observe(
            &ctx,
            subscriber_topics_count as u64,
            &[KeyValue::new("kind", "subscriber")],
        );
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

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NotifyWatchSubscriptions {
    pub watch_subscriptions_auth: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct NotifySubscribe {
    subscription_auth: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct NotifyUpdate {
    update_auth: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct NotifyDelete {
    delete_auth: String,
}
