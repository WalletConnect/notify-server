use {
    super::Account,
    crate::{
        auth::jwt_token,
        error::{self, Error},
        handlers::ClientData,
        state::AppState,
    },
    axum::{extract::State, http::StatusCode, response::IntoResponse, Json},
    base64::Engine,
    chacha20poly1305::{
        aead::{generic_array::GenericArray, Aead},
        consts::U12,
        KeyInit,
    },
    hyper::HeaderMap,
    mongodb::bson::doc,
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

#[derive(Debug, Serialize, Deserialize)]
pub struct Notification {
    title: String,
    body: String,
    icon: String,
    url: String,
}

type Topic = String;

/// Data structure representing PublishParams.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PublishParams {
    /// Topic to publish to.
    pub topic: Topic,
    /// Message to publish.
    pub message: String,
    /// Duration for which the message should be kept in the mailbox if it can't
    /// be delivered, in seconds.
    #[serde(rename = "ttl")]
    pub ttl_secs: u32,
    // #[serde(default, skip_serializing_if = "is_default")]
    /// A label that identifies what type of message is sent based on the RPC
    /// method used.
    pub tag: u32,
    /// A flag that identifies whether the server should trigger a notification
    /// webhook to a client through a push server.
    #[serde(default)]
    pub prompt: bool,
}

#[derive(Serialize, Deserialize)]
pub struct JsonRpcPayload {
    id: String,
    jsonrpc: String,
    method: String,
    params: PublishParams,
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
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(cast_args): Json<CastArgs>,
) -> Result<axum::response::Response, error::Error> {
    // impl IntoResponse {
    let db = state.example_store.clone();

    let project_id = headers.get("Auth").unwrap().to_str().unwrap();

    let notification_json = serde_json::to_string(&cast_args.notification).unwrap();

    // Fetching accounts from db
    let accounts = cast_args
        .accounts
        .into_iter()
        .map(|x| x.0)
        .collect::<Vec<String>>();

    let mut cursor = db
        .collection::<ClientData>(project_id)
        .find(
            // doc! { "project_id":project_id, "id": {"$in": &accounts}},
            doc! { "_id": {"$in": &accounts}},
            None,
        )
        .await?;

    let mut not_found: HashSet<String> = accounts.into_iter().collect();

    let mut clients = HashMap::<String, Vec<(String, String)>>::new();

    while let Some(data) = cursor.try_next().await.unwrap() {
        not_found.remove(&data.id);

        let encryption_key = hex::decode(&data.sym_key).unwrap();
        let encrypted_notification = {
            let cipher =
                chacha20poly1305::ChaCha20Poly1305::new(GenericArray::from_slice(&encryption_key));

            cipher
                .encrypt(
                    // TODO: proper nonce
                    &GenericArray::<u8, U12>::default(),
                    notification_json.clone().as_bytes(),
                )
                .unwrap()
        };
        let base64_notification =
            base64::engine::general_purpose::STANDARD_NO_PAD.encode(encrypted_notification);
        let message = serde_json::to_string(&JsonRpcPayload {
            id: "1".to_string(),
            jsonrpc: "2.0".to_string(),
            method: "irn_publish".to_string(),
            params: PublishParams {
                topic: sha256::digest(&*encryption_key),
                message: base64_notification.clone(),
                ttl_secs: 8400,
                tag: 4002,
                prompt: false,
            },
        })
        .unwrap();
        clients
            .entry(data.relay_url)
            .or_default()
            .push((message, data.id));
    }

    let mut confirmed_sends = HashSet::new();
    let mut failed_sends = HashSet::new();
    for (url, notifications) in clients {
        let token = jwt_token(&url, &state.keypair);
        let relay_query = format!("auth={token}&projectId={project_id}");
        let mut url = url::Url::parse(&url).unwrap();
        url.set_query(Some(&relay_query));

        let mut connection = tungstenite::connect(&url);

        for notification_data in notifications {
            let (encrypted_notification, sender) = notification_data;

            match &mut connection {
                Ok(connection) => {
                    let ws = &mut connection.0;
                    match ws.write_message(tungstenite::Message::Text(encrypted_notification)) {
                        Ok(_) => {
                            confirmed_sends.insert(sender);
                        }
                        Err(e) => {
                            failed_sends.insert((sender, e.to_string()));
                        }
                    };
                }
                Err(e) => {
                    failed_sends.insert((
                        sender,
                        format!(
                            "Failed connecting to {}://{}",
                            &url.scheme(),
                            &url.host().unwrap()
                        ),
                    ));
                }
            }
        }
    }

    // .into_iter()
    // .filter(|account| !confirmed_sends.contains(account))
    // //TODO: Fix this
    // // .filter(|account| !failed_sends.contains(account))
    // .collect();

    // Get them into one struct and serialize as json
    let response = Response {
        sent: confirmed_sends,
        failed: failed_sends,
        not_found,
    };
    let response_json = serde_json::to_string(&response).unwrap();

    Ok((StatusCode::OK, response_json).into_response())
}

#[cfg(test)]
mod tests {
    use chacha20poly1305::{aead::OsRng, KeyInit};

    #[test]
    fn generate_proper_key() {
        let test = chacha20poly1305::ChaCha20Poly1305::generate_key(&mut OsRng);
        let hex = hex::encode(test);
        dbg!(hex);
    }
}
