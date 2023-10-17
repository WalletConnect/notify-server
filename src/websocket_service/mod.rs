use {
    crate::{
        error::Error,
        handlers::subscribe_topic::ProjectData,
        metrics::Metrics,
        spec::{
            NOTIFY_DELETE_TAG, NOTIFY_SUBSCRIBE_TAG, NOTIFY_UPDATE_TAG,
            NOTIFY_WATCH_SUBSCRIPTIONS_TAG,
        },
        state::AppState,
        types::LookupEntry,
        websocket_service::handlers::{
            notify_delete, notify_subscribe, notify_update, notify_watch_subscriptions,
        },
        wsclient::{self, create_connection_opts, RelayClientEvent},
        Result,
    },
    futures::{executor, future, StreamExt},
    mongodb::{bson::doc, Database},
    rand::Rng,
    relay_rpc::{
        domain::{MessageId, Topic},
        rpc::JSON_RPC_VERSION_STR,
    },
    serde::{Deserialize, Serialize},
    sha2::Sha256,
    std::{
        sync::{Arc, Mutex},
        time::{Duration, Instant},
    },
    tokio::time::timeout,
    tracing::{error, info, instrument, warn},
    wc::metrics::otel::Context,
};

pub mod handlers;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestBody {
    pub id: MessageId,
    pub jsonrpc: String,
    pub params: String,
}

#[derive(Debug, Clone)]
pub enum WebSocketClientState {
    Connected,
    Disconnected,
    Connecting(Arc<tokio::sync::Notify>),
}

pub struct WebsocketService {
    app_state: Arc<AppState>,
    wsclient: Arc<relay_client::websocket::Client>,
    client_events: tokio::sync::mpsc::UnboundedReceiver<wsclient::RelayClientEvent>,
    client_state: Arc<Mutex<WebSocketClientState>>,
}

impl WebsocketService {
    pub async fn new(
        app_state: Arc<AppState>,
        wsclient: Arc<relay_client::websocket::Client>,
        rx: tokio::sync::mpsc::UnboundedReceiver<RelayClientEvent>,
    ) -> Result<Self> {
        Ok(Self {
            app_state,
            client_events: rx,
            wsclient,
            client_state: Arc::new(Mutex::new(WebSocketClientState::Disconnected)),
        })
    }

    async fn connect(&mut self) -> Result<()> {
        info!("Initial connection to the relay");
        respawn_connection(
            self.wsclient.clone(),
            self.app_state.clone(),
            self.client_state.clone(),
        )
        .await
    }

    pub async fn run(&mut self) -> Result<()> {
        self.connect().await?;
        loop {
            let Some(msg) = self.client_events.recv().await else {
                return Err(crate::error::Error::RelayClientStopped);
            };
            match msg {
                wsclient::RelayClientEvent::Message(msg) => {
                    let app_state = self.app_state.clone();
                    let wsclient = self.wsclient.clone();
                    let client_state = self.client_state.clone();
                    tokio::spawn({
                        async move { handle_msg(msg, &app_state, &wsclient, &client_state).await }
                    });
                }
                wsclient::RelayClientEvent::Error(e) => {
                    warn!("Received error from relay: {}", e);
                    while let Err(e) = self.connect().await {
                        error!("Error reconnecting to relay: {}", e);
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    }
                }
                wsclient::RelayClientEvent::Disconnected(e) => {
                    info!("Received disconnect from relay: {e:?}");
                    while let Err(e) = self.connect().await {
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
}

#[instrument(skip_all)]
pub async fn respawn_connection(
    wsclient: Arc<relay_client::websocket::Client>,
    app_state: Arc<AppState>,
    client_state: Arc<Mutex<WebSocketClientState>>,
) -> Result<()> {
    info!("Changing state to the `Connecting`");
    let notify = Arc::new(tokio::sync::Notify::new());
    {
        let mut state = client_state.lock().unwrap();
        *state = WebSocketClientState::Connecting(notify.clone());
    }

    info!("Connecting to the relay and resubscribe");
    wsclient
        .connect(&create_connection_opts(
            &app_state.config.relay_url,
            &app_state.config.project_id,
            &app_state.keypair,
            &app_state.config.notify_url,
        )?)
        .await?;
    resubscribe(
        app_state.notify_keys.key_agreement_topic.clone(),
        &app_state.database,
        &wsclient,
        &app_state.metrics,
    )
    .await?;

    info!("Changing state to the `Connected` and notifying waiters");
    {
        let mut state = client_state.lock().unwrap();
        *state = WebSocketClientState::Connected;
    }
    notify.notify_waiters();

    Ok(())
}

#[instrument(skip_all, fields(topic = %topic, tag = %tag, message_id = %sha256::digest(message.as_bytes())))]
pub async fn publish_message(
    wsclient: Arc<relay_client::websocket::Client>,
    wsclient_state: Arc<Mutex<WebSocketClientState>>,
    app_state: Arc<AppState>,
    topic: Topic,
    message: &str,
    tag: u32,
    ttl: Duration,
    prompt: bool,
) -> Result<()> {
    // Checking the current state of the websocket client connection
    let current_client_state_clone;
    {
        let current_state = wsclient_state.lock().unwrap();
        current_client_state_clone = current_state.clone();
    } // immediately drop the lock so we can listen for notify later
    info!(
        "Current state of the websocket client connection before publish: {:?}",
        current_client_state_clone
    );

    match current_client_state_clone {
        WebSocketClientState::Disconnected => {
            error!("WebSocket client is in `Disconnected` state");
            return Err(Error::RelayClientStopped);
        }
        WebSocketClientState::Connecting(notify) => {
            let wait_notify_timeout = Duration::from_secs(10);
            warn!(
                "WebSocket client is in `Connecting` state, waiting notify for {:?} seconds",
                wait_notify_timeout
            );
            match timeout(wait_notify_timeout, notify.clone().notified()).await {
                Ok(_) => info!("WebSocket client is now in `Connected` state (notify is received)"),
                Err(_) => {
                    error!("Timeout waiting for notify, WebSocket client is still not connected");
                    return Err(Error::RelayClientStopped);
                }
            }
        }
        // Do nothing if the client is already connected and proceed to publish
        WebSocketClientState::Connected => {}
    }

    if let Err(e) = wsclient
        .publish(topic.clone(), message, tag, ttl, prompt)
        .await
    {
        warn!("Error publishing message (trying to reconnect): {}", e);
        if let Err(e) =
            respawn_connection(wsclient.clone(), app_state.clone(), wsclient_state.clone()).await
        {
            error!("Error reconnecting to the relay: {}", e);
            return Err(Error::RelayClientStopped);
        };

        info!("Trying to republish message after reconnect");
        if let Err(e) = wsclient.publish(topic, message, tag, ttl, prompt).await {
            error!("Error publishing message (after reconnect): {}", e);
            return Err(Error::RelayClientStopped);
        }
    }
    Ok(())
}

#[instrument(skip_all, fields(topic = %msg.topic, tag = %msg.tag, message_id = %sha256::digest(msg.message.as_bytes())))]
async fn handle_msg(
    msg: relay_client::websocket::PublishedMessage,
    app_state: &Arc<AppState>,
    client: &Arc<relay_client::websocket::Client>,
    client_state: &Arc<Mutex<WebSocketClientState>>,
) {
    let topic = msg.topic.clone();
    let tag = msg.tag;

    info!("Received message");

    match tag {
        NOTIFY_DELETE_TAG => {
            info!("Received notify delete on topic {topic}");
            if let Err(e) = notify_delete::handle(msg, app_state, client, client_state).await {
                warn!("Error handling notify delete: {e}");
            }
            info!("Finished processing notify delete on topic {topic}");
        }
        NOTIFY_SUBSCRIBE_TAG => {
            info!("Received notify subscribe on topic {topic}");
            if let Err(e) = notify_subscribe::handle(msg, app_state, client, client_state).await {
                warn!("Error handling notify subscribe: {e}");
            }
            info!("Finished processing notify subscribe on topic {topic}");
        }
        NOTIFY_UPDATE_TAG => {
            info!("Received notify update on topic {topic}");
            if let Err(e) = notify_update::handle(msg, app_state, client, client_state).await {
                warn!("Error handling notify update: {e}");
            }
            info!("Finished processing notify update on topic {topic}");
        }
        NOTIFY_WATCH_SUBSCRIPTIONS_TAG => {
            info!("Received notify watch subscriptions on topic {topic}");
            if let Err(e) =
                notify_watch_subscriptions::handle(msg, app_state, client, client_state).await
            {
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
    database: &Arc<Database>,
    client: &Arc<relay_client::websocket::Client>,
    metrics: &Option<Metrics>,
) -> Result<()> {
    info!("Resubscribing to all topics");
    let start = Instant::now();

    client.subscribe(key_agreement_topic).await?;

    // TODO pipeline key_agreement_topic and both cursors into 1 iterator and call
    // batch_subscribe in one set of chunks instead of 2

    // Get all topics from db
    let cursor = database
        .collection::<LookupEntry>("lookup_table")
        .find(None, None)
        .await?;

    // Iterate over all topics and sub to them again using the _id field from each
    // record
    // Chunked into 500, as thats the max relay is allowing
    let mut clients_count = 0;
    cursor
        .chunks(relay_rpc::rpc::MAX_SUBSCRIPTION_BATCH_SIZE)
        .for_each(|chunk| {
            clients_count += chunk.len();
            let topics = chunk
                .into_iter()
                .filter_map(|x| x.ok())
                .map(|x| Topic::new(x.topic.into()))
                .collect::<Vec<Topic>>();
            if let Err(e) = executor::block_on(client.batch_subscribe(topics)) {
                warn!("Error resubscribing to topics: {}", e);
            }
            future::ready(())
        })
        .await;
    info!("clients_count: {clients_count}");

    let cursor = database
        .collection::<ProjectData>("project_data")
        .find(None, None)
        .await?;

    let mut projects_count = 0;
    cursor
        .chunks(relay_rpc::rpc::MAX_SUBSCRIPTION_BATCH_SIZE)
        .for_each(|chunk| {
            projects_count += chunk.len();
            let topics = chunk
                .into_iter()
                .filter_map(|x| x.ok())
                .map(|x| Topic::new(x.topic.into()))
                .collect::<Vec<Topic>>();
            if let Err(e) = executor::block_on(client.batch_subscribe(topics)) {
                warn!("Error resubscribing to topics: {}", e);
            }
            future::ready(())
        })
        .await;
    info!("projects_count: {projects_count}");

    if let Some(metrics) = metrics {
        let ctx = Context::current();
        metrics
            .subscribed_project_topics
            .observe(&ctx, projects_count as u64, &[]);
        metrics
            .subscribed_client_topics
            .observe(&ctx, clients_count as u64, &[]);
        metrics.subscribe_latency.record(
            &ctx,
            start.elapsed().as_millis().try_into().unwrap(),
            &[],
        );
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
