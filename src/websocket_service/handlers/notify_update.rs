use {
    super::notify_watch_subscriptions::update_subscription_watchers,
    crate::{
        auth::{
            add_ttl,
            from_jwt,
            sign_jwt,
            verify_identity,
            AuthError,
            Authorization,
            AuthorizedApp,
            SharedClaims,
            SubscriptionUpdateRequestAuth,
            SubscriptionUpdateResponseAuth,
        },
        error::Error,
        handlers::subscribe_topic::ProjectData,
        spec::{NOTIFY_UPDATE_RESPONSE_TAG, NOTIFY_UPDATE_RESPONSE_TTL},
        state::AppState,
        types::{ClientData, Envelope, EnvelopeType0, LookupEntry},
        websocket_service::{
            decode_key,
            handlers::{decrypt_message, retrying_publish},
            NotifyRequest,
            NotifyResponse,
            NotifyUpdate,
        },
        Result,
    },
    base64::Engine,
    chrono::Utc,
    mongodb::bson::doc,
    relay_rpc::domain::DecodedClientId,
    serde_json::{json, Value},
    std::sync::Arc,
};

// TODO test idempotency
pub async fn handle(
    msg: relay_client::websocket::PublishedMessage,
    state: &Arc<AppState>,
    client: &Arc<relay_client::websocket::Client>,
) -> Result<()> {
    let _request_id = uuid::Uuid::new_v4();
    let topic = msg.topic.to_string();

    // Grab record from db
    let lookup_data = state
        .database
        .collection::<LookupEntry>("lookup_table")
        .find_one(doc!("_id":topic.clone()), None)
        .await?
        .ok_or(crate::error::Error::NoProjectDataForTopic(topic.clone()))?;

    let project_data = state
        .database
        .collection::<ProjectData>("project_data")
        .find_one(doc!("_id": lookup_data.project_id.clone()), None)
        .await?
        .ok_or(crate::error::Error::NoProjectDataForTopic(
            topic.to_string(),
        ))?;

    let client_data = state
        .database
        .collection::<ClientData>(&lookup_data.project_id)
        .find_one(doc!("_id": &lookup_data.account), None)
        .await?
        .ok_or(crate::error::Error::NoClientDataForTopic(topic.clone()))?;

    let envelope = Envelope::<EnvelopeType0>::from_bytes(
        base64::engine::general_purpose::STANDARD.decode(msg.message.to_string())?,
    )?;

    let sym_key = decode_key(&client_data.sym_key)?;

    let msg: NotifyRequest<NotifyUpdate> = decrypt_message(envelope, &sym_key)?;

    let sub_auth = from_jwt::<SubscriptionUpdateRequestAuth>(&msg.params.update_auth)?;

    let account = {
        if sub_auth.shared_claims.act != "notify_update" {
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

    let client_data = ClientData {
        id: account.clone(),
        relay_url: state.config.relay_url.clone(),
        sym_key: client_data.sym_key.clone(),
        scope: sub_auth.scp.split(' ').map(|s| s.to_owned()).collect(),
        expiry: sub_auth.shared_claims.exp,
    };

    state
        .register_client(
            &lookup_data.project_id,
            client_data,
            &url::Url::parse(&state.config.relay_url)?,
        )
        .await?;

    let identity = DecodedClientId(decode_key(&project_data.identity_keypair.public_key)?);

    let now = Utc::now();
    let response_message = SubscriptionUpdateResponseAuth {
        shared_claims: SharedClaims {
            iat: now.timestamp() as u64,
            exp: add_ttl(now, NOTIFY_UPDATE_RESPONSE_TTL).timestamp() as u64,
            iss: format!("did:key:{identity}"),
            aud: sub_auth.shared_claims.iss,
            act: "notify_update_response".to_string(),
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

    retrying_publish(
        state,
        client,
        response_topic.into(),
        &base64_notification,
        NOTIFY_UPDATE_RESPONSE_TAG,
        NOTIFY_UPDATE_RESPONSE_TTL,
        false,
    )
    .await?;

    update_subscription_watchers(
        &account,
        &project_data.app_domain,
        &state.database,
        client.as_ref(),
        &state.notify_keys.authentication_secret,
        &state.notify_keys.authentication_public,
    )
    .await?;

    Ok(())
}
