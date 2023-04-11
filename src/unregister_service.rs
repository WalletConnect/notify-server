use {
    crate::{
        auth::jwt_token,
        handlers::notify::Envelope,
        state::AppState,
        types::{ClientData, LookupEntry},
        wsclient::{self, WsClient},
        Result,
    },
    base64::Engine,
    chacha20poly1305::{
        aead::{generic_array::GenericArray, Aead},
        ChaCha20Poly1305,
        KeyInit,
    },
    futures::{executor, future, select, FutureExt, StreamExt},
    mongodb::{bson::doc, Database},
    std::sync::Arc,
    tokio::sync::mpsc::Receiver,
    tracing::{error, info, warn},
    walletconnect_sdk::rpc::rpc::{Params, Payload},
};

#[derive(Debug, Clone)]
pub enum UnregisterMessage {
    Register(String),
}

pub struct UnregisterService {
    state: Arc<AppState>,
    client: WsClient,
    rx: Receiver<UnregisterMessage>,
}

impl UnregisterService {
    pub async fn new(app_state: Arc<AppState>, rx: Receiver<UnregisterMessage>) -> Result<Self> {
        let url = app_state.config.relay_url.clone();

        let mut client = wsclient::connect(
            &app_state.config.relay_url,
            &app_state.config.project_id,
            jwt_token(&url, &app_state.unregister_keypair)?,
        )
        .await?;

        resubscribe(&app_state.database, &mut client).await?;

        Ok(Self {
            rx,
            state: app_state,
            client,
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        loop {
            match self.client.handler.is_finished() {
                true => {
                    warn!("Client handler has finished, spawning new one");
                    self.client = wsclient::connect(
                        &self.state.config.relay_url,
                        &self.state.config.project_id,
                        jwt_token(&self.state.config.relay_url, &self.state.unregister_keypair)?,
                    )
                    .await
                    .unwrap();
                    resubscribe(&self.state.database, &mut self.client).await?;
                }
                false => {
                    select! {

                    msg =  self.rx.recv().fuse() => {
                        if let Some(msg) = msg {
                            let UnregisterMessage::Register(topic) = msg;
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
                                    info!("Unregister service received message: {:?}", msg);
                                    if let Payload::Request(req) = msg {
                                        if let Params::Subscription(params) = req.params {
                                            if params.data.tag == 4004 {
                                                let topic = params.data.topic.to_string();
                                                // TODO: Keep subscription id in db
                                                if let Err(e) =self.client.unsubscribe(&topic, "").await {
                                                    error!("Error unsubscribing Cast from topic: {}", e);
                                                };
                                                match self.state.database.collection::<LookupEntry>("lookup_table").find_one_and_delete(doc! {"_id": &topic }, None).await {

                                                Ok(Some(LookupEntry{ project_id, account, ..}))  =>  {
                                                    match self.state.database.collection::<ClientData>(&project_id).find_one_and_delete(doc! {"_id": &account }, None).await {
                                                        Ok(Some(acc)) => {
                                                            match base64::engine::general_purpose::STANDARD.decode(params.data.message.to_string()) {
                                                                Ok(message_bytes) => {
                                                                    let envelope = Envelope::from_bytes(message_bytes);
                                                                    // Safe unwrap - we are sure that stored keys are valid
                                                                    let encryption_key = hex::decode(&acc.sym_key).unwrap();
                                                                    let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(&encryption_key));

                                                                    match cipher.decrypt(GenericArray::from_slice(&envelope.iv), chacha20poly1305::aead::Payload::from(&*envelope.sealbox)){
                                                                        Ok(msg) => {
                                                                           let msg = String::from_utf8(msg).unwrap();
                                                                            info!("Unregistered {} from {} with reason {}", account, project_id, msg);
                                                                        },
                                                                        Err(e) => {
                                                                            warn!("Unregistered {} from {}, but couldn't decrypt reason data: {}", account, project_id, e);
                                                                        }
                                                                    }
                                                                },
                                                                Err(e) => {
                                                                    warn!("Unregistered {} from {}, but couldn't decode base64 message data: {}", project_id, params.data.message.to_string(), e);
                                                                }
                                                            };
                                                            // ACK unregister message
                                                            self.client.send_ack(req.id).await?;

                                                        },
                                                        Ok(None) => {
                                                            warn!("No entry found for account: {}", &account);
                                                        },
                                                        Err(e) => {
                                                            error!("Error unregistering account {}: {}", &account,  e);
                                                        }
                                                    }
                                                },
                                                Ok(None) => {
                                                    warn!("No entry found for topic: {}", &topic);
                                                },
                                                 Err(e) => {
                                                    error!("Error unregistering from topic {}: {}", &topic,  e);
                                                }
                                            }
                                        }
                                    } else {
                                        self.client.send_ack(req.id).await?;
                                    }
                                }},
                                Err(_) => {
                                    warn!("Client handler has finished, spawning new one");
                                    self.client = wsclient::connect(
                                        &self.state.config.relay_url,
                                        &self.state.config.project_id,
                                        jwt_token(&self.state.config.relay_url, &self.state.unregister_keypair)?,
                                    )
                                    .await?;

                                    resubscribe(&self.state.database, &mut self.client).await?;

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
    // TODO: Sub to all
    info!("Resubscribing to all topics");
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
                error!("Error resubscribing to topics: {}", e);
            }
            future::ready(())
        })
        .await;
    Ok(())
}
