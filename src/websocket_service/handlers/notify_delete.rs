use {
    crate::{
        error::Error,
        state::{AppState, WebhookNotificationEvent},
        types::{ClientData, Envelope, EnvelopeType0, LookupEntry},
        Result,
    },
    anyhow::anyhow,
    base64::Engine,
    chacha20poly1305::{
        aead::{generic_array::GenericArray, Aead},
        ChaCha20Poly1305,
        KeyInit,
    },
    mongodb::bson::doc,
    std::sync::Arc,
    tracing::{info, warn},
};

pub async fn handle(
    msg: relay_client::websocket::PublishedMessage,
    state: &Arc<AppState>,
    client: &Arc<relay_client::websocket::Client>,
) -> Result<()> {
    let request_id = uuid::Uuid::new_v4();
    let topic = msg.topic;
    let database = &state.database;
    let subscription_id = msg.subscription_id;

    let Ok(Some(LookupEntry {
        project_id,
        account,
        ..
    })) = database
        .collection::<LookupEntry>("lookup_table")
        .find_one_and_delete(doc! {"_id": &topic.to_string() }, None)
        .await
    else {
        return Err(Error::NoProjectDataForTopic(topic.to_string()));
    };

    let Ok(Some(acc)) = database
        .collection::<ClientData>(&project_id)
        .find_one_and_delete(doc! {"_id": &account }, None)
        .await
    else {
        return Err(Error::NoClientDataForTopic(topic.to_string()));
    };

    let Ok(message_bytes) =
        base64::engine::general_purpose::STANDARD.decode(msg.message.to_string())
    else {
        return Err(Error::Other(anyhow!("Failed to decode message")));
    };

    let envelope = Envelope::<EnvelopeType0>::from_bytes(message_bytes)?;
    let encryption_key = hex::decode(&acc.sym_key)?;
    let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(&encryption_key));

    let Ok(msg) = cipher.decrypt(
        GenericArray::from_slice(&envelope.iv),
        chacha20poly1305::aead::Payload::from(&*envelope.sealbox),
    ) else {
        warn!(
            "[{request_id}] Unregistered {} from {}, but couldn't decrypt message",
            account, project_id
        );
        return Err(Error::EncryptionError(format!(
            "[{request_id}] Failed to decrypt"
        )));
    };

    let msg = String::from_utf8(msg)?;
    info!(
        "[{request_id}] Unregistered {} from {} with reason {}",
        account, project_id, msg
    );
    if let Err(e) = client.unsubscribe(topic.clone(), subscription_id).await {
        warn!(
            "[{request_id}] Error unsubscribing Notify from topic: {}",
            e
        );
    };

    state
        .notify_webhook(
            &project_id,
            WebhookNotificationEvent::Unsubscribed,
            &account,
        )
        .await?;

    Ok(())
}
