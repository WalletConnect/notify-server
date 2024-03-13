use {
    super::error::RelayMessageClientError,
    crate::{rpc::JsonRpcRequest, types::Envelope},
    chacha20poly1305::{aead::Aead, ChaCha20Poly1305, KeyInit},
    serde::de::DeserializeOwned,
    sha2::digest::generic_array::GenericArray,
};

pub mod notify_delete;
pub mod notify_get_notifications;
pub mod notify_subscribe;
pub mod notify_update;
pub mod notify_watch_subscriptions;

fn decrypt_message<T: DeserializeOwned, E>(
    envelope: Envelope<E>,
    encryption_key: &[u8; 32],
) -> Result<JsonRpcRequest<T>, RelayMessageClientError> {
    let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(encryption_key));

    let msg = cipher
        .decrypt(
            GenericArray::from_slice(&envelope.iv),
            chacha20poly1305::aead::Payload::from(&*envelope.sealbox),
        )
        .map_err(RelayMessageClientError::DecryptionError)?;

    serde_json::from_slice::<JsonRpcRequest<T>>(&msg)
        .map_err(RelayMessageClientError::JsonDeserialization)
}
