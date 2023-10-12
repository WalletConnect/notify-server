use {
    crate::{
        metrics::Metrics,
        model::helpers::{get_project_topics, get_subscriber_topics},
        spec::{
            NOTIFY_DELETE_TAG,
            NOTIFY_SUBSCRIBE_TAG,
            NOTIFY_UPDATE_TAG,
            NOTIFY_WATCH_SUBSCRIPTIONS_TAG,
        },
        state::AppState,
        websocket_service::handlers::{
            notify_delete,
            notify_subscribe,
            notify_update,
            notify_watch_subscriptions,
        },
        wsclient::{self, create_connection_opts, RelayClientEvent},
        Result,
    },
    rand::Rng,
    relay_rpc::{
        domain::{MessageId, Topic},
        rpc::JSON_RPC_VERSION_STR,
    },
    serde::{Deserialize, Serialize},
    sha2::Sha256,
    sqlx::PgPool,
    std::{sync::Arc, time::Instant},
    tracing::{error, info, warn},
    uuid::Uuid,
    wc::metrics::otel::Context,
};

pub mod handlers;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestBody {
    pub id: MessageId,
    pub jsonrpc: String,
    pub params: String,
}

pub struct WebsocketService {
    state: Arc<AppState>,
    wsclient: Arc<relay_client::websocket::Client>,
    client_events: tokio::sync::mpsc::UnboundedReceiver<wsclient::RelayClientEvent>,
}

impl WebsocketService {
    pub async fn new(
        app_state: Arc<AppState>,
        wsclient: Arc<relay_client::websocket::Client>,
        rx: tokio::sync::mpsc::UnboundedReceiver<RelayClientEvent>,
    ) -> Result<Self> {
        Ok(Self {
            state: app_state,
            client_events: rx,
            wsclient,
        })
    }

    async fn connect(&mut self) -> Result<()> {
        self.wsclient
            .connect(&create_connection_opts(
                &self.state.config.relay_url,
                &self.state.config.project_id,
                &self.state.keypair,
                &self.state.config.notify_url,
            )?)
            .await?;

        resubscribe(
            self.state.notify_keys.key_agreement_topic.clone(),
            &self.state.postgres,
            &self.wsclient,
            &self.state.metrics,
        )
        .await?;
        Ok(())
    }

    pub async fn run(&mut self) -> Result<()> {
        self.connect().await?;
        loop {
            let Some(msg) = self.client_events.recv().await else {
                return Err(crate::error::Error::RelayClientStopped);
            };
            match msg {
                wsclient::RelayClientEvent::Message(msg) => {
                    handle_msg(msg, &self.state, &self.wsclient).await;
                }
                wsclient::RelayClientEvent::Error(e) => {
                    warn!("Received error from relay: {}", e);
                    while let Err(e) = self.connect().await {
                        error!("Error reconnecting to relay: {}", e);
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    }
                }
                wsclient::RelayClientEvent::Disconnected(_) => {
                    while let Err(e) = self.connect().await {
                        warn!("Error reconnecting to relay: {}", e);
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    }
                }
                wsclient::RelayClientEvent::Connected => {
                    info!("Connected to relay");
                }
            }
        }
    }
}

async fn handle_msg(
    msg: relay_client::websocket::PublishedMessage,
    state: &Arc<AppState>,
    client: &Arc<relay_client::websocket::Client>,
) {
    let request_id = Uuid::new_v4();
    let topic = msg.topic.clone();
    let tag = msg.tag;
    let message_id = sha256::digest(msg.message.as_bytes());
    let _span = tracing::info_span!(
        "handle_msg", request_id = %request_id, topic = %topic, tag = %tag, message_id = %message_id,
    )
    .entered();

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
            if let Err(e) = notify_subscribe::handle(msg, state, client).await {
                warn!("Error handling notify subscribe: {e}");
            }
            info!("Finished processing notify subscribe on topic {topic}");
        }
        NOTIFY_UPDATE_TAG => {
            info!("Received notify update on topic {topic}");
            if let Err(e) = notify_update::handle(msg, state, client).await {
                warn!("Error handling notify update: {e}");
            }
            info!("Finished processing notify update on topic {topic}");
        }
        NOTIFY_WATCH_SUBSCRIPTIONS_TAG => {
            info!("Received notify watch subscriptions on topic {topic}");
            if let Err(e) = notify_watch_subscriptions::handle(msg, state, client).await {
                warn!("Error handling notify watch subscriptions: {e}");
            }
            info!("Finished processing notify watch subscriptions on topic {topic}");
        }
        _ => {
            info!("Ignored tag {tag} on topic {topic}");
        }
    }
}

async fn resubscribe(
    key_agreement_topic: Topic,
    postgres: &PgPool,
    client: &Arc<relay_client::websocket::Client>,
    metrics: &Option<Metrics>,
) -> Result<()> {
    info!("Resubscribing to all topics");
    let start = Instant::now();

    client.subscribe(key_agreement_topic).await?;

    // Chunked into 500, as thats the max relay is allowing

    let subscribers = get_subscriber_topics(postgres).await?;
    let subscribers_count = subscribers.len();
    for chunk in subscribers.chunks(relay_rpc::rpc::MAX_SUBSCRIPTION_BATCH_SIZE) {
        client.batch_subscribe(chunk).await?;
    }
    info!("subscribers_count: {subscribers_count}");

    let projects = get_project_topics(postgres).await?;
    let projects_count = projects.len();
    for chunk in projects.chunks(relay_rpc::rpc::MAX_SUBSCRIPTION_BATCH_SIZE) {
        client.batch_subscribe(chunk).await?;
    }
    info!("projects_count: {projects_count}");

    if let Some(metrics) = metrics {
        let ctx = Context::current();
        metrics
            .subscribed_project_topics
            .observe(&ctx, projects_count as u64, &[]);
        metrics
            .subscribed_client_topics
            .observe(&ctx, subscribers_count as u64, &[]);
        metrics
            .subscribe_latency
            .record(&ctx, start.elapsed().as_millis().try_into().unwrap(), &[]);
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
