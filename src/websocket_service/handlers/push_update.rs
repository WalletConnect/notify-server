use {
    crate::{
        auth::SubscriptionAuth,
        log::info,
        state::AppState,
        types::{ClientData, Envelope, EnvelopeType0, LookupEntry},
        websocket_service::{NotifyMessage, NotifyResponse, NotifySubscribe},
        Result,
    },
    base64::Engine,
    chacha20poly1305::{
        aead::{generic_array::GenericArray, Aead},
        ChaCha20Poly1305,
        KeyInit,
    },
    mongodb::bson::doc,
    std::{sync::Arc, time::Duration},
};

pub async fn handle(
    msg: relay_client::websocket::PublishedMessage,
    state: &Arc<AppState>,
    client: &Arc<relay_client::websocket::Client>,
) -> Result<()> {
    let request_id = uuid::Uuid::new_v4();
    let topic = msg.topic.to_string();

    // Grab record from db
    let lookup_data = state
        .database
        .collection::<LookupEntry>("lookup_table")
        .find_one(doc!("_id":topic.clone()), None)
        .await?
        .ok_or(crate::error::Error::NoProjectDataForTopic(topic.clone()))?;
    info!("[{request_id}] Fetched data for topic: {:?}", &lookup_data);

    let client_data = state
        .database
        .collection::<ClientData>(&lookup_data.project_id)
        .find_one(doc!("_id": lookup_data.account), None)
        .await?
        .ok_or(crate::error::Error::NoClientDataForTopic(topic.clone()))?;

    info!("[{request_id}] Fetched client: {:?}", &client_data);

    let envelope = Envelope::<EnvelopeType0>::from_bytes(
        base64::engine::general_purpose::STANDARD.decode(msg.message.to_string())?,
    )?;

    let encryption_key = hex::decode(client_data.sym_key.clone())?;

    let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(&encryption_key));

    let msg = cipher
        .decrypt(
            GenericArray::from_slice(&envelope.iv),
            chacha20poly1305::aead::Payload::from(&*envelope.sealbox),
        )
        .map_err(|_| crate::error::Error::EncryptionError("Failed to decrypt".into()))?;

    let msg: NotifyMessage<NotifySubscribe> = serde_json::from_slice(&msg)?;

    let sub_auth = SubscriptionAuth::from_jwt(&msg.params.subscription_auth)?;

    let response = NotifyResponse::<bool> {
        id: msg.id,
        jsonrpc: "2.0".into(),
        result: true,
    };
    info!("[{request_id}] Decrypted response");

    let envelope = Envelope::<EnvelopeType0>::new(&client_data.sym_key, response)?;
    let base64_notification = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    client
        .publish(
            topic.into(),
            base64_notification,
            4009,
            Duration::from_secs(86400),
            false,
        )
        .await?;

    let client_data = ClientData {
        id: sub_auth.sub.trim_start_matches("did:pkh:").into(),
        relay_url: state.config.relay_url.clone(),
        sym_key: client_data.sym_key.clone(),
        scope: sub_auth.scp.split(' ').map(|s| s.into()).collect(),
    };
    info!("[{request_id}] Updating client: {:?}", &client_data);

    state
        .register_client(
            &lookup_data.project_id,
            &client_data,
            &url::Url::parse(&state.config.relay_url)?,
        )
        .await?;

    Ok(())
}
