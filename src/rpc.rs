use {
    once_cell::sync::Lazy,
    relay_client::MessageIdGenerator,
    relay_rpc::{domain::MessageId, rpc::JSON_RPC_VERSION},
    serde::{Deserialize, Serialize},
    sha2::Sha256,
    std::{array::TryFromSliceError, sync::Arc},
    thiserror::Error,
};

#[derive(Debug, Error)]
pub enum DecodeKeyError {
    #[error("Hex decode error: {0}")]
    HexDecodeError(#[from] hex::FromHexError),

    #[error("TryFromSliceError: {0}")]
    TryFromSliceError(#[from] TryFromSliceError),

    #[error("Input too short")]
    InputTooShortError,
}

pub fn decode_key(key: &str) -> Result<[u8; 32], DecodeKeyError> {
    Ok(hex::decode(key)?
        .get(..32)
        .ok_or(DecodeKeyError::InputTooShortError)?
        .try_into()?)
}

#[derive(Debug, Error)]
pub enum DeriveKeyError {
    #[error("Hkdf invalid length: {0}")]
    HkdfInvalidLength(hkdf::InvalidLength),
}

pub fn derive_key(
    public_key: &x25519_dalek::PublicKey,
    private_key: &x25519_dalek::StaticSecret,
) -> Result<[u8; 32], DeriveKeyError> {
    let shared_key = private_key.diffie_hellman(public_key);

    let derived_key = hkdf::Hkdf::<Sha256>::new(None, shared_key.as_bytes());

    let mut expanded_key = [0u8; 32];
    derived_key
        .expand(b"", &mut expanded_key)
        .map_err(DeriveKeyError::HkdfInvalidLength)?;
    Ok(expanded_key)
}

static MESSAGE_ID_GENERATOR: Lazy<MessageIdGenerator> = Lazy::new(MessageIdGenerator::new);

#[derive(Serialize, Deserialize, Debug)]
pub struct JsonRpcRequest<T> {
    pub id: MessageId,
    pub jsonrpc: Arc<str>,
    pub method: String,
    pub params: T,
}

impl<T> JsonRpcRequest<T> {
    pub fn new(method: &str, params: T) -> Self {
        Self {
            id: MESSAGE_ID_GENERATOR.next(),
            jsonrpc: JSON_RPC_VERSION.clone(),
            method: method.to_owned(),
            params,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct JsonRpcResponse<T> {
    pub id: MessageId,
    pub jsonrpc: Arc<str>,
    pub result: T,
}

impl<T> JsonRpcResponse<T> {
    pub fn new(id: MessageId, result: T) -> Self {
        Self {
            id,
            jsonrpc: JSON_RPC_VERSION.clone(),
            result,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct JsonRpcResponseError {
    pub id: MessageId,
    pub jsonrpc: Arc<str>,
    pub error: JsonRpcError,
}

impl JsonRpcResponseError {
    pub fn new(id: MessageId, error: JsonRpcError) -> Self {
        Self {
            id,
            jsonrpc: JSON_RPC_VERSION.clone(),
            error,
        }
    }
}

// https://www.jsonrpc.org/specification#error_object
#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i16,
    pub message: String,
    // pub data: Value,
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotifyMessageAuth {
    pub message_auth: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AuthMessage {
    pub auth: String,
}
