use {
    super::Account,
    crate::{
        auth::jwt_token,
        error::{self},
        handlers::ClientData,
        jsonrpc::{JsonRpcParams, JsonRpcPayload, Notification, PublishParams},
        state::AppState,
    },
    axum::{
        extract::{Path, State},
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
    rand::{distributions::Uniform, prelude::Distribution},
    rand_core::OsRng,
    serde::{Deserialize, Serialize},
    std::{
        collections::{HashMap, HashSet},
        sync::Arc,
    },
    tokio_stream::StreamExt,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct CastArgs {
    notification: Notification,
    accounts: Vec<Account>,
}

#[derive(Serialize)]
#[repr(C)]
struct Envelope<'a> {
    envelope_type: u8,
    iv: [u8; 12],
    sealbox: &'a [u8],
}

impl<'a> Envelope<'a> {
    fn to_bytes(&self) -> Vec<u8> {
        let mut serialized = vec![];
        serialized.push(self.envelope_type);
        serialized.extend_from_slice(&self.iv);
        serialized.extend_from_slice(self.sealbox);
        serialized
    }
}

// Change String to Account
// Change String to Error
#[derive(Serialize, Deserialize)]
struct Response {
    sent: HashSet<String>,
    failed: HashSet<(String, String)>,
    not_found: HashSet<String>,
}

pub async fn handler(
    Path(project_id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(cast_args): Json<CastArgs>,
) -> Result<axum::response::Response, error::Error> {
    let db = state.example_store.clone();

    let mut confirmed_sends = HashSet::new();
    let mut failed_sends = HashSet::new();

    let message = serde_json::to_string(&JsonRpcPayload {
        id: "1".to_string(),
        jsonrpc: "2.0".to_string(),
        // method: "wc_pushMessage".to_string(),
        params: JsonRpcParams::Push(cast_args.notification),
    })?;

    // Fetching accounts from db
    let accounts = cast_args
        .accounts
        .into_iter()
        .map(|x| x.0)
        .collect::<Vec<String>>();

    let mut cursor = db
        .collection::<ClientData>(&project_id)
        .find(doc! { "_id": {"$in": &accounts}}, None)
        .await?;

    let mut not_found: HashSet<String> = accounts.into_iter().collect();

    let mut clients = HashMap::<String, Vec<(String, String)>>::new();

    let mut rng = OsRng {};
    let uniform = Uniform::from(0u8..=255);

    while let Some(data) = cursor.try_next().await.unwrap() {
        not_found.remove(&data.id);

        let encryption_key = hex::decode(&data.sym_key).unwrap();
        let encrypted_notification = {
            let cipher =
                chacha20poly1305::ChaCha20Poly1305::new(GenericArray::from_slice(&encryption_key));

            // TODO: proper nonce
            let nonce: GenericArray<u8, U12> =
                GenericArray::from_iter(uniform.sample_iter(&mut rng).take(12));

            let encrypted = match cipher.encrypt(&nonce, message.clone().as_bytes()) {
                Err(_) => {
                    failed_sends.insert((data.id, "Failed to encrypt the payload".to_string()));
                    continue;
                }
                Ok(ciphertext) => ciphertext,
            };

            let envelope = Envelope {
                envelope_type: 0,
                iv: nonce.into(),
                sealbox: &encrypted,
            };

            envelope.to_bytes()
        };

        let base64_notification =
            base64::engine::general_purpose::STANDARD_NO_PAD.encode(encrypted_notification);

        let message = serde_json::to_string(&JsonRpcPayload {
            id: "1".to_string(),
            jsonrpc: "2.0".to_string(),
            // method: "irn_publish".to_string(),
            params: JsonRpcParams::Publish(PublishParams {
                topic: sha256::digest(&*encryption_key),
                message: base64_notification.clone(),
                ttl_secs: 8400,
                tag: 4002,
                prompt: false,
            }),
        })?;

        clients
            .entry(data.relay_url)
            .or_default()
            .push((message, data.id));
    }

    for (url, notifications) in clients {
        let token = jwt_token(&url, &state.keypair);
        let relay_query = format!("auth={token}&projectId={project_id}");

        let mut url = url::Url::parse(&url)?;
        url.set_query(Some(&relay_query));

        let mut connection = tungstenite::connect(&url);

        for notification_data in notifications {
            let (encrypted_notification, sender) = notification_data;

            match &mut connection {
                Ok(connection) => {
                    let ws = &mut connection.0;
                    dbg!(&encrypted_notification);
                    match ws.write_message(tungstenite::Message::Text(encrypted_notification)) {
                        Ok(_) => {
                            confirmed_sends.insert(sender);
                        }
                        Err(e) => {
                            failed_sends.insert((sender, e.to_string()));
                        }
                    };
                }
                Err(_) => {
                    failed_sends.insert((
                        sender,
                        // Formatting this instead of just giving the whole URL to avoid leaking
                        // project_id
                        format!(
                            "Failed connecting to {}://{}",
                            &url.scheme(),
                            // Safe unwrap since all stored urls are "wss://", for which host
                            // always exists
                            &url.host().unwrap()
                        ),
                    ));
                }
            }
        }
    }

    // Get them into one struct and serialize as json
    let response = Response {
        sent: confirmed_sends,
        failed: failed_sends,
        not_found,
    };

    Ok((StatusCode::OK, Json(response)).into_response())
}

#[cfg(test)]
mod tests {
    use chacha20poly1305::KeyInit;

    #[test]
    fn generate_proper_key() {
        let test =
            chacha20poly1305::ChaCha20Poly1305::generate_key(&mut chacha20poly1305::aead::OsRng {});
        let hex = hex::encode(test);
        dbg!(hex);
    }
}
