use {
    super::NotifyMessage,
    crate::{error::Error, types::Envelope},
    chacha20poly1305::{aead::Aead, ChaCha20Poly1305, KeyInit},
    serde::de::DeserializeOwned,
    sha2::digest::generic_array::GenericArray,
};

pub mod push_delete;
pub mod push_subscribe;
pub mod push_update;

fn decrypt_message<T: DeserializeOwned, E>(
    envelope: Envelope<E>,
    key: &str,
) -> crate::error::Result<NotifyMessage<T>> {
    let encryption_key = hex::decode(key)?;

    let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(&encryption_key));

    let msg = cipher
        .decrypt(
            GenericArray::from_slice(&envelope.iv),
            chacha20poly1305::aead::Payload::from(&*envelope.sealbox),
        )
        .map_err(|_| crate::error::Error::EncryptionError("Failed to decrypt".into()))?;

    serde_json::from_slice::<NotifyMessage<T>>(&msg).map_err(Error::SerdeJson)
}
