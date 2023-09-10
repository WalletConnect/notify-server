use {
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
            SubscriptionRequestAuth,
            SubscriptionResponseAuth,
        },
        error::Error,
        handlers::subscribe_topic::ProjectData,
        spec::{NOTIFY_SUBSCRIBE_RESPONSE_TAG, NOTIFY_SUBSCRIBE_RESPONSE_TTL},
        state::AppState,
        types::{ClientData, Envelope, EnvelopeType0, EnvelopeType1},
        websocket_service::{
            decode_key,
            derive_key,
            handlers::{decrypt_message, notify_watch_subscriptions::update_subscription_watchers},
            NotifyRequest,
            NotifyResponse,
            NotifySubscribe,
        },
        Result,
    },
    base64::Engine,
    chrono::Utc,
    mongodb::bson::doc,
    relay_rpc::domain::DecodedClientId,
    serde_json::{json, Value},
    std::{sync::Arc, time::Duration},
    tracing::info,
    x25519_dalek::{PublicKey, StaticSecret},
};

// TODO test idempotency (create subscriber a second time for the same account)
pub async fn handle(
    msg: relay_client::websocket::PublishedMessage,
    state: &Arc<AppState>,
    client: &Arc<relay_client::websocket::Client>,
) -> Result<()> {
    let request_id = uuid::Uuid::new_v4();
    let topic = msg.topic.to_string();

    // Grab record from db
    let project_data = state
        .database
        .collection::<ProjectData>("project_data")
        .find_one(doc!("topic": topic.clone()), None)
        .await?
        .ok_or(crate::error::Error::NoProjectDataForTopic(topic))?;

    let envelope = Envelope::<EnvelopeType1>::from_bytes(
        base64::engine::general_purpose::STANDARD.decode(msg.message.to_string())?,
    )?;

    let client_pubkey = envelope.pubkey();
    let client_pubkey = x25519_dalek::PublicKey::from(client_pubkey);

    let sym_key = derive_key(
        &client_pubkey,
        &x25519_dalek::StaticSecret::from(decode_key(&project_data.signing_keypair.private_key)?),
    )?;

    let msg: NotifyRequest<NotifySubscribe> = decrypt_message(envelope, &sym_key)?;

    let id = msg.id;

    let sub_auth = from_jwt::<SubscriptionRequestAuth>(&msg.params.subscription_auth)?;
    let sub_auth_hash = sha256::digest(msg.params.subscription_auth);

    let account = {
        if sub_auth.act != "notify_subscription" {
            return Err(AuthError::InvalidAct)?;
        }

        let Authorization { account, app } =
            verify_identity(&sub_auth.shared_claims.iss, &sub_auth.ksu, &sub_auth.sub).await?;

        // TODO verify `sub_auth.aud` matches `project_data.identity_keypair`

        if let AuthorizedApp::Limited(app) = app {
            if app != project_data.app_domain()? {
                Err(Error::AppSubscriptionsUnauthorized)?;
            }
        }

        // TODO merge code with integration.rs#verify_jwt()
        //      - put desired `iss` value as an argument to make sure we verify it

        account
    };

    let secret = StaticSecret::random_from_rng(chacha20poly1305::aead::OsRng);
    let public = PublicKey::from(&secret);

    let identity = DecodedClientId(decode_key(&project_data.identity_keypair.public_key)?);

    let identity_public = DecodedClientId(public.to_bytes());

    let now = Utc::now();
    let response_message = SubscriptionResponseAuth {
        shared_claims: SharedClaims {
            iat: now.timestamp() as u64,
            exp: add_ttl(now, NOTIFY_SUBSCRIBE_RESPONSE_TTL).timestamp() as u64,
            iss: format!("did:key:{identity}"),
        },
        aud: sub_auth.shared_claims.iss,
        act: "notify_subscription_response".to_string(),
        sub: format!("did:key:{identity_public}"),
        app: project_data.dapp_url.to_string(),
    };
    let response_auth = sign_jwt(
        response_message,
        &ed25519_dalek::SigningKey::from_bytes(&decode_key(
            &project_data.identity_keypair.private_key,
        )?),
    )?;

    let response = NotifyResponse::<Value> {
        id,
        jsonrpc: "2.0".into(),
        result: json!({ "responseAuth": response_auth }), // TODO use structure
    };

    let notify_key = derive_key(&client_pubkey, &secret)?;

    let envelope = Envelope::<EnvelopeType0>::new(&sym_key, response)?;

    let base64_notification = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    let response_topic = sha256::digest(&sym_key);

    let client_data = ClientData {
        id: account.clone(),
        relay_url: state.config.relay_url.clone(),
        sym_key: hex::encode(notify_key),
        scope: sub_auth.scp.split(' ').map(|s| s.to_owned()).collect(),
        expiry: sub_auth.shared_claims.exp,
        sub_auth_hash,
    };

    let notify_topic = sha256::digest(&notify_key);

    // Registers account and subscribes to topic
    info!(
        "[{request_id}] Registering account: {:?} with topic: {} at project: {}. Scope: {:?}. Msg \
         id: {:?}",
        &client_data.id, &notify_topic, &project_data.id, &client_data.scope, &msg.id,
    );
    state
        .register_client(
            &project_data.id,
            client_data,
            &url::Url::parse(&state.config.relay_url)?,
        )
        .await?;

    // Send noop to extend ttl of relay's mapping
    client
        .publish(
            notify_topic.clone().into(),
            "",
            4050,
            Duration::from_secs(300),
            false,
        )
        .await?;

    client
        .publish(
            response_topic.into(),
            base64_notification,
            NOTIFY_SUBSCRIBE_RESPONSE_TAG,
            NOTIFY_SUBSCRIBE_RESPONSE_TTL,
            false,
        )
        .await?;

    update_subscription_watchers(
        &account,
        &project_data.dapp_url,
        &state.database,
        client.as_ref(),
        &state.notify_keys.authentication_secret,
        &state.notify_keys.authentication_public,
    )
    .await?;

    Ok(())
}
