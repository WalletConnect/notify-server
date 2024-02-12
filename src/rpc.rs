use {
    crate::error::NotifyServerError,
    rand::Rng,
    relay_rpc::{domain::MessageId, rpc::JSON_RPC_VERSION_STR},
    serde::{Deserialize, Serialize},
    sha2::Sha256,
};

pub fn decode_key(key: &str) -> Result<[u8; 32], NotifyServerError> {
    Ok(hex::decode(key)?
        .get(..32)
        .ok_or(NotifyServerError::InputTooShortError)?
        .try_into()?)
}

pub fn derive_key(
    public_key: &x25519_dalek::PublicKey,
    private_key: &x25519_dalek::StaticSecret,
) -> Result<[u8; 32], NotifyServerError> {
    let shared_key = private_key.diffie_hellman(public_key);

    let derived_key = hkdf::Hkdf::<Sha256>::new(None, shared_key.as_bytes());

    let mut expanded_key = [0u8; 32];
    derived_key
        .expand(b"", &mut expanded_key)
        .map_err(NotifyServerError::HkdfInvalidLength)?;
    Ok(expanded_key)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestBody {
    pub id: MessageId,
    pub jsonrpc: String,
    pub params: String,
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

impl<T> NotifyResponse<T> {
    pub fn new(id: u64, result: T) -> Self {
        NotifyResponse {
            id,
            jsonrpc: JSON_RPC_VERSION_STR.to_owned(),
            result,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ResponseAuth {
    pub response_auth: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NotifyWatchSubscriptions {
    pub watch_subscriptions_auth: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NotifySubscriptionsChanged {
    pub subscriptions_changed_auth: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NotifySubscribe {
    pub subscription_auth: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NotifyUpdate {
    pub update_auth: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NotifyDelete {
    pub delete_auth: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AuthMessage {
    pub auth: String,
}
