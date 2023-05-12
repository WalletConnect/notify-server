use {
    crate::{
        auth::jwt_token,
        handlers::subscribe_topic::ProjectData,
        log::{info, warn},
        state::AppState,
        types::LookupEntry,
        websocket_service::handlers::{push_delete, push_subscribe, push_update},
        wsclient::{self, WsClient},
        Result,
    },
    futures::{executor, future, select, FutureExt, StreamExt},
    log::debug,
    mongodb::{bson::doc, Database},
    serde::{Deserialize, Serialize},
    sha2::Sha256,
    std::sync::Arc,
    tokio::sync::mpsc::Receiver,
    walletconnect_sdk::rpc::{
        domain::MessageId,
        rpc::{Params, Payload},
    },
};

mod handlers;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestBody {
    pub id: MessageId,
    pub jsonrpc: String,
    pub params: String,
}

#[derive(Debug, Clone)]
pub enum WebsocketMessage {
    Register(String),
}

pub struct WebsocketService {
    state: Arc<AppState>,
    client: WsClient,
    rx: Receiver<WebsocketMessage>,
}

impl WebsocketService {
    pub async fn new(app_state: Arc<AppState>, rx: Receiver<WebsocketMessage>) -> Result<Self> {
        let url = app_state.config.relay_url.clone();

        let mut client = wsclient::connect(
            &app_state.config.relay_url,
            &app_state.config.project_id,
            jwt_token(&url, &app_state.webclient_keypair)?,
        )
        .await?;

        resubscribe(&app_state.database, &mut client).await?;

        Ok(Self {
            rx,
            state: app_state,
            client,
        })
    }

    async fn reconnect(&mut self) -> Result<()> {
        let url = self.state.config.relay_url.clone();
        let jwt = jwt_token(&url, &self.state.webclient_keypair)?;
        self.client = wsclient::connect(&url, &self.state.config.project_id, jwt).await?;
        resubscribe(&self.state.database, &mut self.client).await?;
        Ok(())
    }

    pub async fn run(&mut self) -> Result<()> {
        loop {
            match self.client.handler.is_finished() {
                true => {
                    warn!("Client handler has finished, spawning new one");
                    self.reconnect().await?;
                }
                false => {
                    select! {

                    msg =  self.rx.recv().fuse() => {
                        if let Some(msg) = msg {
                            let WebsocketMessage::Register(topic) = msg;
                                info!("Subscribing to topic: {}", topic);
                                if let Err(e) = self.client.subscribe(&topic).await {
                                    warn!("Error subscribing to topic: {}", e);
                                }
                          }
                    }
                        ,
                        message = self.client.recv().fuse() => {
                            match message {
                                Ok(msg) => {
                                    let msg_id = msg.id();
                                    if let Err(e) = self.client.send_ack(msg_id).await {
                                        warn!("Failed responding to message: {} with err: {}", msg_id, e);
                                    };
                                    if let Err(e) = handle_msg(msg, &self.state, &mut self.client).await {
                                        warn!("Error handling message: {}", e);
                                    }

                                },
                                Err(_) => {
                                    warn!("Client handler has finished, spawning new one");
                                    self.reconnect().await?;
                                }
                            }



                        }
                    };
                }
            }
        }
    }
}

async fn resubscribe(database: &Arc<Database>, client: &mut WsClient) -> Result<()> {
    debug!("Resubscribing to all topics");
    // Get all topics from db
    let cursor = database
        .collection::<LookupEntry>("lookup_table")
        .find(None, None)
        .await?;

    // Iterate over all topics and sub to them again using the _id field from each
    // record
    // Chunked into 500, as thats the max relay is allowing
    cursor
        .chunks(500)
        .for_each(|chunk| {
            let topics = chunk
                .into_iter()
                .filter_map(|x| x.ok())
                .map(|x| x.topic)
                .collect::<Vec<String>>();
            if let Err(e) = executor::block_on(client.batch_subscribe(topics)) {
                warn!("Error resubscribing to topics: {}", e);
            }
            future::ready(())
        })
        .await;

    let cursor = database
        .collection::<ProjectData>("project_data")
        .find(None, None)
        .await?;

    cursor
        .chunks(500)
        .for_each(|chunk| {
            let topics = chunk
                .into_iter()
                .filter_map(|x| x.ok())
                .map(|x| x.topic)
                .collect::<Vec<String>>();
            if let Err(e) = executor::block_on(client.batch_subscribe(topics)) {
                warn!("Error resubscribing to topics: {}", e);
            }
            future::ready(())
        })
        .await;
    Ok(())
}

async fn handle_msg(msg: Payload, state: &Arc<AppState>, client: &mut WsClient) -> Result<()> {
    info!("Websocket service received message: {:?}", msg);
    if let Payload::Request(req) = msg {
        if let Params::Subscription(params) = req.params {
            match params.data.tag {
                4004 => {
                    let topic = params.data.topic.clone();
                    info!("Received push delete for topic: {}", topic);
                    push_delete::handle(params, state, client).await?;
                    info!("Finished processing push delete for topic: {}", topic);
                }
                4006 => {
                    let topic = params.data.topic.clone();
                    info!("Received push subscribe on topic: {}", &topic);
                    push_subscribe::handle(params, state, client).await?;
                    info!("Finished processing push subscribe for topic: {}", topic);
                }
                4008 => {
                    let topic = params.data.topic.clone();
                    info!("Received push update on topic: {}", &topic);
                    push_update::handle(params, state, client).await?;
                    info!("Finished processing push update for topic: {}", topic);
                }
                _ => {
                    info!("Ignored tag: {}", params.data.tag);
                }
            }
        } else {
            info!("Ignored request: {:?}", req);
        }
    }
    Ok(())
}

fn derive_key(pubkey: String, privkey: String) -> Result<String> {
    let pubkey: [u8; 32] = hex::decode(pubkey)?[..32].try_into()?;
    let privkey: [u8; 32] = hex::decode(privkey)?[..32].try_into()?;

    let secret_key = x25519_dalek::StaticSecret::from(privkey);
    let public_key = x25519_dalek::PublicKey::from(pubkey);

    let shared_key = secret_key.diffie_hellman(&public_key);

    let derived_key = hkdf::Hkdf::<Sha256>::new(None, shared_key.as_bytes());

    let mut expanded_key = [0u8; 32];
    derived_key
        .expand(b"", &mut expanded_key)
        .map_err(|_| crate::error::Error::HkdfInvalidLength)?;
    Ok(hex::encode(expanded_key))
}

#[derive(Serialize, Deserialize, Debug)]
pub struct NotifyMessage<T> {
    pub id: u64,
    pub jsonrpc: String,
    pub params: T,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct NotifyResponse<T> {
    pub id: u64,
    pub jsonrpc: String,
    pub result: T,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct NotifySubscribe {
    subscription_auth: String,
}
