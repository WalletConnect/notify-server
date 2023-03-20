use {
    crate::{auth::jwt_token, error::Result},
    dashmap::DashMap,
    futures::{future, StreamExt},
    rand::{thread_rng, Rng},
    std::sync::Arc,
    tokio::{select, sync::mpsc},
    tokio_stream::wrappers::ReceiverStream,
    tracing::{error, info, warn},
    tungstenite::Message,
    walletconnect_sdk::rpc::{
        auth::ed25519_dalek::Keypair,
        rpc::{Params, Payload, Publish, Request, Subscribe, SuccessfulResponse, Unsubscribe},
    },
};

type MessageId = String;

#[derive(Debug)]
pub struct WsClient {
    pub project_id: String,
    pub tx: mpsc::Sender<Message>,
    pub rx: mpsc::Receiver<Result<Message>>,
    pub handler: tokio::task::JoinHandle<()>,
    /// Received ACKs, contains a set of message IDs.
    pub received_acks: Arc<DashMap<MessageId, serde_json::Value>>,
}

impl WsClient {
    pub fn is_finished(&self) -> bool {
        self.handler.is_finished()
    }

    pub async fn reconnect(&mut self, url: &str, keypair: &Keypair) -> Result<()> {
        let jwt = jwt_token(url, keypair)?;
        connect(url, &self.project_id, jwt).await?;
        Ok(())
    }

    pub async fn send(&mut self, msg: Request) -> Result<()> {
        self.send_raw(Payload::Request(msg)).await
    }

    pub async fn send_ack(&mut self, id: walletconnect_sdk::rpc::domain::MessageId) -> Result<()> {
        self.send_raw(Payload::Response(
            walletconnect_sdk::rpc::rpc::Response::Success(SuccessfulResponse::new(
                id,
                serde_json::Value::Bool(true),
            )),
        ))
        .await
    }

    pub async fn send_raw(&mut self, msg: Payload) -> Result<()> {
        let msg = serde_json::to_string(&msg).unwrap();
        self.tx
            .send(Message::Text(msg))
            .await
            .map_err(|_| crate::error::Error::ChannelClosed)
    }

    pub async fn recv(&mut self) -> Result<Payload> {
        loop {
            match self.rx.recv().await {
                Some(msg) => match msg? {
                    Message::Text(msg) => return Ok(serde_json::from_str(&msg).unwrap()),
                    Message::Ping(_) => {
                        info!("Received ping from Relay WS, sending pong");
                        self.pong().await?;
                    }
                    e => {
                        error!("{:?}", e);
                    }
                },
                None => {
                    return Err(crate::error::Error::ChannelClosed);
                }
            }
        }
    }

    async fn pong(&mut self) -> Result<()> {
        let msg = Message::Pong("heartbeat".into());
        self.tx
            .send(msg)
            .await
            .map_err(|_| crate::error::Error::ChannelClosed)
    }

    pub async fn publish(&mut self, topic: &str, payload: &str) -> Result<()> {
        self.publish_with_tag(topic, payload, 1000).await
    }

    pub async fn publish_with_tag(&mut self, topic: &str, payload: &str, tag: u32) -> Result<()> {
        let msg = Payload::Request(new_rpc_request(
            walletconnect_sdk::rpc::rpc::Params::Publish(Publish {
                topic: topic.into(),
                message: payload.into(),
                ttl_secs: 8400,
                tag,
                prompt: true,
            }),
        ));

        self.send_raw(msg).await
    }

    pub async fn subscribe(&mut self, topic: &str) -> Result<()> {
        let msg = Payload::Request(new_rpc_request(Params::Subscribe(Subscribe {
            topic: topic.into(),
        })));
        self.send_raw(msg).await
    }

    pub async fn unsubscribe(&mut self, topic: &str, subscription_id: &str) -> Result<()> {
        let msg = Payload::Request(new_rpc_request(Params::Unsubscribe(Unsubscribe {
            topic: topic.into(),
            subscription_id: subscription_id.into(),
        })));
        self.send_raw(msg).await
    }
}

fn new_rpc_request(params: Params) -> Request {
    Request {
        id: thread_rng().gen::<u64>().into(),
        jsonrpc: "2.0".into(),
        params,
    }
}

pub async fn connect(url: &str, project_id: &str, jwt: String) -> Result<WsClient> {
    info!("Connecting to Relay WS ({})...", url);
    let relay_query = format!("auth={jwt}&projectId={project_id}");

    let mut url = url::Url::parse(url)?;
    url.set_query(Some(&relay_query));

    let (connection, _) = async_tungstenite::tokio::connect_async(&url).await?;

    // A channel for passing messages to websocket
    let (wr_tx, wr_rx) = mpsc::channel(1024);

    // A channel for incoming messages from websocket
    let (rd_tx, rd_rx) = mpsc::channel::<Result<_>>(1024);

    let handler = tokio::spawn(async move {
        let (tx, rx) = connection.split();
        // Forward messages to the write half
        let write = ReceiverStream::new(wr_rx).map(Ok).forward(tx);

        // Process incoming messages and close the
        // client in case of a close frame. Else forward them.
        let read = rx
            .take_while(|result| match result {
                Err(_) => future::ready(false),
                Ok(m) => future::ready(!m.is_close()),
            })
            .for_each_concurrent(None, |result| async {
                if let Err(e) = rd_tx.send(result.map_err(Into::into)).await {
                    warn!("WSClient send error: {}", e);
                };
            });

        // Keep the thread alive until either
        // read or write complete.
        select! {
            _ = read => { info!("WSClient read died");  },
            _ = write => { info!("WSClient write died"); },
        };
    });

    Ok(WsClient {
        project_id: project_id.to_string(),
        tx: wr_tx,
        rx: rd_rx,
        handler,
        received_acks: Default::default(),
    })
}
