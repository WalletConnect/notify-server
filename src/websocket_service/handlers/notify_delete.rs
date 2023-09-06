use {
    crate::{
        auth::{
            add_ttl,
            from_jwt,
            sign_jwt,
            verify_identity,
            AuthError,
            SharedClaims,
            SubscriptionDeleteResponseAuth,
            SubscruptionDeleteRequestAuth,
        },
        error::Error,
        handlers::subscribe_topic::ProjectData,
        spec::{NOTIFY_DELETE_RESPONSE_TAG, NOTIFY_DELETE_RESPONSE_TTL},
        state::{AppState, WebhookNotificationEvent},
        types::{ClientData, Envelope, EnvelopeType0, LookupEntry},
        websocket_service::{
            handlers::decrypt_message,
            NotifyDelete,
            NotifyMessage,
            NotifyResponse,
        },
        Result,
    },
    anyhow::anyhow,
    base64::Engine,
    chrono::Utc,
    mongodb::bson::doc,
    relay_rpc::domain::{ClientId, DecodedClientId},
    serde_json::{json, Value},
    std::sync::Arc,
    tracing::{info, warn},
};

// TODO test idempotency
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

    // TODO move above find_one_and_delete()
    let sub_auth = from_jwt::<SubscruptionDeleteRequestAuth>(&msg.params.delete_auth)?;
    let _sub_auth_hash = sha256::digest(msg.params.delete_auth);

    verify_identity(&sub_auth.shared_claims.iss, &sub_auth.ksu, &sub_auth.sub).await?;

    // TODO verify `sub_auth.aud` matches `project_data.identity_keypair`

    // TODO verify `sub_auth.app` matches `project_data.dapp_url`

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

    let now = Utc::now();
    let response_message = SubscriptionDeleteResponseAuth {
        shared_claims: SharedClaims {
            iat: now.timestamp() as u64,
            exp: add_ttl(now, NOTIFY_DELETE_RESPONSE_TTL).timestamp() as u64,
            iss: format!("did:key:{identity}"),
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
        result: json!({ "responseAuth": response_auth }), // TODO use structure
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
            NOTIFY_DELETE_RESPONSE_TAG,
            NOTIFY_DELETE_RESPONSE_TTL,
            false,
        )
        .await?;

    Ok(())
}
