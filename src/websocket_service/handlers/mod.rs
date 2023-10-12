use {
    super::NotifyRequest,
    crate::{
        error::Error,
        state::AppState,
        types::Envelope,
        wsclient::create_connection_opts,
        Result,
    },
    chacha20poly1305::{aead::Aead, ChaCha20Poly1305, KeyInit},
    relay_rpc::domain::Topic,
    serde::de::DeserializeOwned,
    sha2::digest::generic_array::GenericArray,
    std::time::Duration,
    tracing::{error, warn},
};

pub mod notify_delete;
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

async fn retrying_publish(
    state: &AppState,
    client: &relay_client::websocket::Client,
    topic: Topic,
    message: &str,
    tag: u32,
    ttl: Duration,
    prompt: bool,
) -> Result<()> {
    if let Err(e) = client
        .publish(topic.clone(), message, tag, ttl, prompt)
        .await
    {
        warn!("Reconnecting after an error in publishing message: {}", e);
        while let Err(e) = client
            .connect(&create_connection_opts(
                &state.config.relay_url,
                &state.config.project_id,
                &state.keypair,
                &state.config.notify_url,
            )?)
            .await
        {
            error!("Error reconnecting to relay: {}", e);
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        if let Err(e) = client.publish(topic, message, tag, ttl, prompt).await {
            error!("Error on publishing after reconnect, giving up: {}", e);
            return Err(Error::RelayClientStopped);
        }
    }
    Ok(())
}
