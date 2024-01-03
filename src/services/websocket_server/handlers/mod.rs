use {
    super::NotifyRequest,
    crate::{error::Error, types::Envelope},
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
) -> crate::error::Result<NotifyRequest<T>> {
    let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(encryption_key));

    let msg = cipher
        .decrypt(
            GenericArray::from_slice(&envelope.iv),
            chacha20poly1305::aead::Payload::from(&*envelope.sealbox),
        )
        .map_err(|_| crate::error::Error::EncryptionError("Failed to decrypt".into()))?;

    serde_json::from_slice::<NotifyRequest<T>>(&msg).map_err(Error::SerdeJson)
}
