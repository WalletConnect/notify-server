use {
    crate::{
        auth::jwt_token,
        error::{self},
        jsonrpc::{JsonRpcParams, JsonRpcPayload, Notification, PublishParams},
        state::AppState,
        types::ClientData,
    },
    axum::{
        extract::{ConnectInfo, Path, State},
        http::StatusCode,
        response::IntoResponse,
        Json,
    },
    base64::Engine,
    chacha20poly1305::{
        aead::{generic_array::GenericArray, Aead},
        consts::U12,
        KeyInit,
    },
    mongodb::bson::doc,
    opentelemetry::{Context, KeyValue},
    rand::{distributions::Uniform, prelude::Distribution},
    rand_core::OsRng,
    serde::{Deserialize, Serialize},
    std::{
        collections::{HashMap, HashSet},
        net::SocketAddr,
        sync::Arc,
        time::SystemTime,
    },
    tokio_stream::StreamExt,
    tracing::{debug, error, info},
};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NotifyBody {
    pub notification: Notification,
    pub accounts: Vec<String>,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
pub struct SendFailure {
    pub account: String,
    pub reason: String,
}

#[derive(Serialize)]
pub struct Envelope {
    pub envelope_type: u8,
    pub iv: [u8; 12],
    pub sealbox: Vec<u8>,
}

impl Envelope {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut serialized = vec![];
        serialized.push(self.envelope_type);
        serialized.extend_from_slice(&self.iv);
        serialized.extend_from_slice(&self.sealbox);
        serialized
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self {
            envelope_type: bytes[0],
            iv: bytes[1..13].try_into().unwrap(),
            sealbox: bytes[13..].to_vec(),
        }
    }
}

// Change String to Account
// Change String to Error
#[derive(Serialize, Deserialize, Debug)]
pub struct Response {
    pub sent: HashSet<String>,
    pub failed: HashSet<SendFailure>,
    pub not_found: HashSet<String>,
}

pub async fn handler(
    ConnectInfo(_addr): ConnectInfo<SocketAddr>,
    Path(project_id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(cast_args): Json<NotifyBody>,
) -> Result<axum::response::Response, error::Error> {
    let timer = std::time::Instant::now();
    let db = state.database.clone();
    let mut rng = OsRng {};

    let mut confirmed_sends = HashSet::new();
    let mut failed_sends: HashSet<SendFailure> = HashSet::new();

    let id = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    let message = serde_json::to_string(&JsonRpcPayload {
        id,
        jsonrpc: "2.0".to_string(),
        params: JsonRpcParams::Push(cast_args.notification),
    })?;

    // Fetching accounts from db
    let accounts = cast_args.accounts;

    let mut cursor = db
        .collection::<ClientData>(&project_id)
        .find(doc! { "_id": {"$in": &accounts}}, None)
        .await?;

    let mut not_found: HashSet<String> = accounts.into_iter().collect();

    let mut clients = HashMap::<String, Vec<(String, String)>>::new();

    let uniform = Uniform::from(0u8..=255);

    while let Some(data) = cursor.try_next().await.unwrap() {
        not_found.remove(&data.id);

        let encryption_key = hex::decode(&data.sym_key).unwrap();
        let encrypted_notification = {
            let cipher =
                chacha20poly1305::ChaCha20Poly1305::new(GenericArray::from_slice(&encryption_key));

            let nonce: GenericArray<u8, U12> =
                GenericArray::from_iter(uniform.sample_iter(&mut rng).take(12));

            let encrypted = match cipher.encrypt(&nonce, message.clone().as_bytes()) {
                Err(_) => {
                    failed_sends.insert(SendFailure {
                        account: data.id,
                        reason: "Failed to encrypt the payload".to_string(),
                    });
                    continue;
                }
                Ok(ciphertext) => ciphertext,
            };

            let envelope = Envelope {
                envelope_type: 0,
                iv: nonce.into(),
                sealbox: encrypted,
            };

            envelope.to_bytes()
        };

        let base64_notification =
            base64::engine::general_purpose::STANDARD.encode(encrypted_notification);

        let id = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let message = serde_json::to_string(&JsonRpcPayload {
            id,
            jsonrpc: "2.0".to_string(),
            params: JsonRpcParams::Publish(PublishParams {
                topic: sha256::digest(&*encryption_key),
                message: base64_notification.clone(),
                ttl_secs: 86400,
                tag: 4002,
                prompt: true,
            }),
        })?;

        clients
            .entry(data.relay_url)
            .or_default()
            .push((message, data.id));
    }

    for (url, notifications) in clients {
        let token = jwt_token(&url, &state.keypair)?;
        let relay_query = format!("projectId={project_id}&auth={token}");

        let mut url = url::Url::parse(&url)?;
        url.set_query(Some(&relay_query));
        let mut connection = tungstenite::connect(&url);

        for notification_data in notifications {
            let (encrypted_notification, sender) = notification_data;

            match &mut connection {
                Ok(connection) => {
                    let ws = &mut connection.0;
                    match ws.write_message(tungstenite::Message::Text(encrypted_notification)) {
                        Ok(_) => {
                            info!("Casting to client");
                            confirmed_sends.insert(sender);
                        }
                        Err(e) => {
                            failed_sends.insert(SendFailure {
                                account: sender,
                                reason: e.to_string(),
                            });
                        }
                    };
                }
                Err(e) => {
                    error!("{}", e);
                    failed_sends.insert(SendFailure {
                        account: sender,
                        reason: format!(
                            "Failed connecting to {}://{}",
                            &url.scheme(),
                            // Safe unwrap since all stored urls are "wss://", for which host
                            // always exists
                            &url.host().unwrap()
                        ),
                    });
                }
            }
        }
    }

    if let Some(metrics) = &state.metrics {
        let ctx = Context::current();
        metrics
            .dispatched_notifications
            .add(&ctx, confirmed_sends.len() as u64, &[
                KeyValue::new("type", "sent"),
                KeyValue::new("project_id", project_id.clone()),
            ]);

        metrics
            .dispatched_notifications
            .add(&ctx, failed_sends.len() as u64, &[
                KeyValue::new("type", "failed"),
                KeyValue::new("project_id", project_id.clone()),
            ]);

        metrics
            .dispatched_notifications
            .add(&ctx, not_found.len() as u64, &[
                KeyValue::new("type", "not_found"),
                KeyValue::new("project_id", project_id.clone()),
            ]);

        metrics
            .send_latency
            .record(&ctx, timer.elapsed().as_millis().try_into().unwrap(), &[
                KeyValue::new("project_id", project_id.clone()),
            ])
    }

    // Get them into one struct and serialize as json
    let response = Response {
        sent: confirmed_sends,
        failed: failed_sends,
        not_found,
    };

    debug!(
        "Response: {:?} for notify from project: {}",
        response, project_id
    );

    Ok((StatusCode::OK, Json(response)).into_response())
}
