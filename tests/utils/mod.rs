use {
    base64::Engine,
    chrono::Utc,
    notify_server::{
        auth::{AuthError, GetSharedClaims, SharedClaims},
        error::NotifyServerError,
        model::types::AccountId,
        notify_message::NotifyMessage,
        relay_client_helpers::create_http_client,
    },
    relay_client::http::Client,
    relay_rpc::{
        auth::ed25519_dalek::{Signer, SigningKey as Ed25519SigningKey, VerifyingKey},
        domain::{DecodedClientId, ProjectId, Topic},
        jwt::{JwtHeader, JWT_HEADER_ALG, JWT_HEADER_TYP},
        rpc::SubscriptionData,
    },
    reqwest::Response,
    serde::Serialize,
    serde_json::json,
    std::{sync::Arc, time::Duration},
    tokio::sync::{
        broadcast::{error::RecvError, Receiver},
        RwLock,
    },
    tracing::info,
    url::Url,
};

pub mod http_api;
pub mod notify_relay_api;
pub mod relay_api;

pub const RELAY_MESSAGE_DELIVERY_TIMEOUT: Duration = Duration::from_secs(5);

pub const JWT_LEEWAY: i64 = 30;

pub struct RelayClient {
    pub client: Arc<Client>,
    pub receiver: Receiver<SubscriptionData>,
    pub topics: Arc<RwLock<Vec<Topic>>>,
}

impl Clone for RelayClient {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            receiver: self.receiver.resubscribe(),
            topics: self.topics.clone(),
        }
    }
}

const RETRIES: usize = 5;

#[allow(dead_code)]
impl RelayClient {
    pub async fn new(relay_url: Url, relay_project_id: ProjectId, notify_url: Url) -> Self {
        let client = create_http_client(
            &Ed25519SigningKey::generate(&mut rand::thread_rng()),
            relay_url,
            notify_url,
            relay_project_id,
        )
        .unwrap();

        let (tx, rx) = tokio::sync::broadcast::channel(8);
        let topics = Arc::new(RwLock::new(vec![]));
        tokio::task::spawn({
            let relay_client = client.clone();
            let topics = topics.clone();
            async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    let topics = topics.read().await.clone();
                    if topics.is_empty() {
                        continue;
                    }

                    let result = relay_client.batch_fetch(topics.clone()).await;
                    if let Ok(res) = result {
                        for msg in res.messages {
                            if tx.send(msg).is_err() {
                                break;
                            }
                        }
                    }
                }
            }
        });

        Self {
            client: Arc::new(client),
            receiver: rx,
            topics,
        }
    }

    pub async fn subscribe(&self, topic: Topic) {
        self.topics.write().await.push(topic.clone());
        let mut tries = 0;
        loop {
            tries += 1;
            let result = self.client.subscribe_blocking(topic.clone()).await;
            match result {
                Ok(_) => return,
                e if tries > RETRIES => {
                    let _ = e.unwrap();
                }
                _ => {}
            }
        }
    }

    pub async fn publish(
        &self,
        topic: Topic,
        message: impl Into<Arc<str>>,
        tag: u32,
        ttl: Duration,
    ) {
        let message = message.into();
        let mut tries = 0;
        loop {
            tries += 1;
            let result = self
                .client
                .publish(topic.clone(), message.clone(), tag, ttl, false)
                .await;
            println!("publishing {tag}");
            match result {
                Ok(_) => return,
                e if tries > RETRIES => e.unwrap(),
                _ => {}
            }
        }
    }

    pub async fn accept_message(&mut self, tag: u32, topic: &Topic) -> SubscriptionData {
        let result = tokio::time::timeout(RELAY_MESSAGE_DELIVERY_TIMEOUT, async {
            loop {
                let msg = match self.receiver.recv().await {
                    Ok(msg) => msg,
                    Err(RecvError::Closed) => panic!("Receiver closed"),
                    Err(RecvError::Lagged(c)) => {
                        println!("Rceiver lagged by {c} messages; remaining messages:");
                        loop {
                            let next_message_fut =
                                tokio::time::timeout(Duration::from_secs(1), self.receiver.recv())
                                    .await;
                            let remaining_message = match next_message_fut {
                                Ok(msg) => msg,
                                Err(_) => break,
                            };
                            println!("- {remaining_message:?}")
                        }
                        panic!("Receiver lagged");
                    }
                };
                if msg.tag == tag && &msg.topic == topic {
                    return msg;
                } else {
                    info!(
                        "Ignored message {} on topic {}. Expected message {} on topic {}",
                        msg.tag, msg.topic, tag, topic
                    );
                }
            }
        })
        .await;

        match result {
            Ok(msg) => msg,
            Err(_) => panic!("Timeout waiting for {tag} message on topic {topic}"),
        }
    }
}

// Workaround https://github.com/rust-lang/rust-clippy/issues/11613
#[allow(clippy::needless_return_with_question_mark)]
pub fn verify_jwt(jwt: &str, key: &VerifyingKey) -> Result<NotifyMessage, NotifyServerError> {
    // Refactor to call from_jwt() and then check `iss` with:
    // let pub_key = did_key.parse::<DecodedClientId>()?;
    // let key = jsonwebtoken::DecodingKey::from_ed_der(pub_key.as_ref());
    // Or perhaps do the opposite (i.e. serialize key into iss)

    let key = jsonwebtoken::DecodingKey::from_ed_der(key.as_bytes());

    let mut parts = jwt.rsplitn(2, '.');

    let (Some(signature), Some(message)) = (parts.next(), parts.next()) else {
        return Err(AuthError::Format)?;
    };

    // Finally, verify signature.
    let sig_result = jsonwebtoken::crypto::verify(
        signature,
        message.as_bytes(),
        &key,
        jsonwebtoken::Algorithm::EdDSA,
    );

    match sig_result {
        Ok(true) => Ok(serde_json::from_slice::<NotifyMessage>(
            &base64::engine::general_purpose::STANDARD_NO_PAD
                .decode(jwt.split('.').nth(1).unwrap())
                .unwrap(),
        )?),
        Ok(false) | Err(_) => Err(AuthError::InvalidSignature)?,
    }
}

pub fn encode_auth<T: Serialize>(auth: &T, signing_key: &Ed25519SigningKey) -> String {
    let data = JwtHeader {
        typ: JWT_HEADER_TYP,
        alg: JWT_HEADER_ALG,
    };
    let header = serde_json::to_string(&data).unwrap();
    let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(header);

    let claims = {
        let json = serde_json::to_string(auth).unwrap();
        base64::engine::general_purpose::STANDARD_NO_PAD.encode(json)
    };

    let message = format!("{header}.{claims}");

    let signature = signing_key.sign(message.as_bytes());
    let signature = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature.to_bytes());

    format!("{message}.{signature}")
}

#[derive(Debug, Clone, Serialize)]
pub struct UnregisterIdentityRequestAuth {
    #[serde(flatten)]
    pub shared_claims: SharedClaims,
    /// corresponding blockchain account (did:pkh)
    pub pkh: String,
}

impl GetSharedClaims for UnregisterIdentityRequestAuth {
    fn get_shared_claims(&self) -> &SharedClaims {
        &self.shared_claims
    }
}

pub async fn unregister_identity_key(
    keys_server_url: Url,
    account: &AccountId,
    identity_signing_key: &Ed25519SigningKey,
    identity_did_key: &DecodedClientId,
) {
    let unregister_auth = UnregisterIdentityRequestAuth {
        shared_claims: SharedClaims {
            iat: Utc::now().timestamp() as u64,
            exp: Utc::now().timestamp() as u64 + 3600,
            iss: identity_did_key.to_did_key(),
            aud: keys_server_url.to_string(),
            act: "unregister_identity".to_owned(),
            mjv: "0".to_owned(),
        },
        pkh: account.to_did_pkh(),
    };
    let unregister_auth = encode_auth(&unregister_auth, identity_signing_key);
    reqwest::Client::new()
        .delete(keys_server_url.join("/identity").unwrap())
        .body(serde_json::to_string(&json!({"idAuth": unregister_auth})).unwrap())
        .send()
        .await
        .unwrap();
}

pub async fn assert_successful_response(response: Response) -> Response {
    let status = response.status();
    if !status.is_success() {
        panic!(
            "non-successful response {status}: {:?}",
            response.text().await
        );
    }
    response
}
