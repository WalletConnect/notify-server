use {
    crate::{
        auth::{from_jwt, AuthError, UpdateAuth},
        state::AppState,
        types::{ClientData, Envelope, EnvelopeType0, LookupEntry},
        websocket_service::{
            handlers::decrypt_message,
            NotifyMessage,
            NotifyResponse,
            NotifyUpdate,
        },
        Result,
    },
    base64::Engine,
    mongodb::bson::doc,
    std::{sync::Arc, time::Duration},
    tracing::info,
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

    let msg: NotifyMessage<NotifyUpdate> = decrypt_message(envelope, &client_data.sym_key)?;

    let sub_auth = from_jwt::<UpdateAuth>(&msg.params.update_auth)?;
    let sub_auth_hash = sha256::digest(msg.params.update_auth);

    if sub_auth.act != "notify_update" {
        return Err(AuthError::InvalidAct)?;
    }

    let response = NotifyResponse::<bool> {
        id: msg.id,
        jsonrpc: "2.0".into(),
        result: true, // TODO signed JWT
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
        sub_auth_hash,
        expiry: sub_auth.shared_claims.exp,
        ksu: sub_auth.shared_claims.ksu,
    };
    info!("[{request_id}] Updating client: {:?}", &client_data);

    state
        .register_client(
            &lookup_data.project_id,
            client_data,
            &url::Url::parse(&state.config.relay_url)?,
        )
        .await?;

    Ok(())
}
