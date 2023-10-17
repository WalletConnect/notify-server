use {
    crate::{
        auth::{
            add_ttl, from_jwt, sign_jwt, verify_identity, AuthError, Authorization, AuthorizedApp,
            SharedClaims, SubscriptionDeleteRequestAuth, SubscriptionDeleteResponseAuth,
        },
        error::Error,
        handlers::subscribe_topic::ProjectData,
        spec::{NOTIFY_DELETE_RESPONSE_TAG, NOTIFY_DELETE_RESPONSE_TTL},
        state::{AppState, WebhookNotificationEvent},
        types::{ClientData, Envelope, EnvelopeType0, LookupEntry},
        websocket_service::{
            decode_key,
            handlers::{decrypt_message, notify_watch_subscriptions::update_subscription_watchers},
            publish_message, NotifyDelete, NotifyRequest, NotifyResponse, WebSocketClientState,
        },
        Result,
    },
    anyhow::anyhow,
    base64::Engine,
    chrono::Utc,
    mongodb::bson::doc,
    relay_rpc::domain::DecodedClientId,
    serde_json::{json, Value},
    std::sync::{Arc, Mutex},
    tracing::{info, warn},
};

// TODO make and test idempotency
pub async fn handle(
    msg: relay_client::websocket::PublishedMessage,
    state: &Arc<AppState>,
    client: &Arc<relay_client::websocket::Client>,
    client_state: &Arc<Mutex<WebSocketClientState>>,
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

    let Ok(Some(client_data)) = database
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

    let sym_key = decode_key(&client_data.sym_key)?;

    let msg: NotifyRequest<NotifyDelete> = decrypt_message(envelope, &sym_key)?;

    // TODO move above find_one_and_delete()
    let sub_auth = from_jwt::<SubscriptionDeleteRequestAuth>(&msg.params.delete_auth)?;
    if sub_auth
        .app
        .strip_prefix("did:web:")
        .ok_or(Error::AppNotDidWeb)?
        != project_data.app_domain
    {
        Err(Error::AppDoesNotMatch)?;
    }

    let account = {
        if sub_auth.shared_claims.act != "notify_delete" {
            return Err(AuthError::InvalidAct)?;
        }

        let Authorization { account, app } =
            verify_identity(&sub_auth.shared_claims.iss, &sub_auth.ksu, &sub_auth.sub).await?;

        // TODO verify `sub_auth.aud` matches `project_data.identity_keypair`

        if let AuthorizedApp::Limited(app) = app {
            if app != project_data.app_domain {
                Err(Error::AppSubscriptionsUnauthorized)?;
            }
        }

        account
    };

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

    let identity = DecodedClientId(decode_key(&project_data.identity_keypair.public_key)?);

    let now = Utc::now();
    let response_message = SubscriptionDeleteResponseAuth {
        shared_claims: SharedClaims {
            iat: now.timestamp() as u64,
            exp: add_ttl(now, NOTIFY_DELETE_RESPONSE_TTL).timestamp() as u64,
            iss: format!("did:key:{identity}"),
            aud: sub_auth.shared_claims.iss,
            act: "notify_delete_response".to_string(),
        },
        sub: format!("did:pkh:{account}"),
        app: format!("did:web:{}", project_data.app_domain),
    };
    let response_auth = sign_jwt(
        response_message,
        &ed25519_dalek::SigningKey::from_bytes(&decode_key(
            &project_data.identity_keypair.private_key,
        )?),
    )?;

    let response = NotifyResponse::<Value> {
        id: msg.id,
        jsonrpc: "2.0".into(),
        result: json!({ "responseAuth": response_auth }), // TODO use structure
    };

    let envelope = Envelope::<EnvelopeType0>::new(&sym_key, response)?;

    let base64_notification = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    let response_topic = sha256::digest(&sym_key);

    publish_message(
        client.clone(),
        client_state.clone(),
        state.clone(),
        response_topic.into(),
        &base64_notification,
        NOTIFY_DELETE_RESPONSE_TAG,
        NOTIFY_DELETE_RESPONSE_TTL,
        false,
    )
    .await?;

    update_subscription_watchers(
        &account,
        &project_data.app_domain,
        &state.database,
        client,
        client_state,
        state,
        &state.notify_keys.authentication_secret,
        &state.notify_keys.authentication_public,
    )
    .await?;

    Ok(())
}
