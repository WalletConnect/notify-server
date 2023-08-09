use {
    crate::{
        auth::{from_jwt, sign_jwt, AuthError, DeleteAuth, DeleteResponseAuth, SharedClaims},
        error::Error,
        handlers::subscribe_topic::ProjectData,
        state::{AppState, WebhookNotificationEvent},
        types::{ClientData, Envelope, EnvelopeType0, LookupEntry},
        websocket_service::{
            handlers::{decrypt_message, notify_subscribe::RESPONSE_TTL},
            NotifyDelete,
            NotifyMessage,
            NotifyResponse,
        },
        Result,
    },
    anyhow::anyhow,
    base64::Engine,
    mongodb::bson::doc,
    relay_rpc::domain::{ClientId, DecodedClientId},
    serde_json::{json, Value},
    std::{sync::Arc, time::Duration},
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

    let project_data = state
        .database
        .collection::<ProjectData>("project_data")
        .find_one(doc!("_id": project_id.clone()), None)
        .await?
        .ok_or(crate::error::Error::NoProjectDataForTopic(
            topic.to_string(),
        ))?;

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

    let msg: NotifyMessage<NotifyDelete> = decrypt_message(envelope, &acc.sym_key)?;

    let sub_auth = from_jwt::<DeleteAuth>(&msg.params.delete_auth)?;
    let _sub_auth_hash = sha256::digest(msg.params.delete_auth);

    if sub_auth.act != "notify_delete" {
        return Err(AuthError::InvalidAct)?;
    }

    info!(
        "[{request_id}] Unregistered {} from {} with reason {}",
        account, project_id, sub_auth.sub,
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

    // response

    let decoded_client_id = DecodedClientId(
        hex::decode(project_data.identity_keypair.public_key.clone())?[0..32].try_into()?,
    );
    let identity = ClientId::from(decoded_client_id).to_string();

    let response_message = DeleteResponseAuth {
        shared_claims: SharedClaims {
            iat: chrono::Utc::now().timestamp() as u64,
            exp: (chrono::Utc::now() + chrono::Duration::seconds(RESPONSE_TTL as i64)).timestamp()
                as u64,
            iss: format!("did:key:{identity}"),
            ksu: sub_auth.shared_claims.ksu.clone(),
        },
        aud: sub_auth.shared_claims.iss,
        act: "notify_delete_response".to_string(),
        sub: acc.sub_auth_hash,
        app: project_data.dapp_url.to_string(),
    };
    let response_auth = sign_jwt(response_message, &project_data.identity_keypair)?;

    let response = NotifyResponse::<Value> {
        id: msg.id,
        jsonrpc: "2.0".into(),
        result: json!({ "responseAuth": response_auth }),
    };
    info!(
        "[{request_id}] Response for user: {}",
        serde_json::to_string(&response)?
    );

    let response_sym_key = acc.sym_key;
    let envelope = Envelope::<EnvelopeType0>::new(&response_sym_key, response)?;

    let base64_notification = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    let key = hex::decode(response_sym_key)?;
    let response_topic = sha256::digest(&*key);
    info!("[{request_id}] Response_topic: {}", &response_topic);

    client
        .publish(
            response_topic.into(),
            base64_notification,
            4005,
            Duration::from_secs(86400),
            false,
        )
        .await?;

    Ok(())
}
