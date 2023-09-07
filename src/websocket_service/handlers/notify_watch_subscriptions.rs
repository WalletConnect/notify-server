use {
    crate::{
        auth::{
            add_ttl,
            from_jwt,
            sign_jwt,
            verify_identity,
            AuthError,
            SharedClaims,
            WatchSubscriptionsRequestAuth,
            WatchSubscriptionsResponseAuth,
        },
        error::Error,
        spec::{
            NOTIFY_SUBSCRIBE_RESPONSE_TTL,
            NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_TAG,
            NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_TTL,
        },
        state::AppState,
        types::{Envelope, EnvelopeType0, EnvelopeType1},
        websocket_service::{
            derive_key,
            handlers::decrypt_message,
            NotifyMessage,
            NotifyResponse,
            NotifyWatchSubscriptions,
        },
        Result,
    },
    base64::Engine,
    chrono::Utc,
    relay_rpc::domain::DecodedClientId,
    serde_json::{json, Value},
    std::sync::Arc,
};

// TODO test idempotency (create subscriber a second time for the same account)
pub async fn handle(
    msg: relay_client::websocket::PublishedMessage,
    state: &Arc<AppState>,
    client: &Arc<relay_client::websocket::Client>,
) -> Result<()> {
    let _request_id = uuid::Uuid::new_v4();

    {
        if msg.topic != state.notify_keys.key_agreement_topic {
            return Err(Error::WrongNotifyWatchSubscriptionsTopic(msg.topic));
        }
    }

    // // Grab record from db
    // let project_data = state
    //     .database
    //     .collection::<ProjectData>("project_data")
    //     .find_one(doc!("topic": topic.clone()), None)
    //     .await?
    //     .ok_or(crate::error::Error::NoProjectDataForTopic(topic))?;

    let envelope = Envelope::<EnvelopeType1>::from_bytes(
        base64::engine::general_purpose::STANDARD.decode(msg.message.to_string())?,
    )?;

    let client_public_key = x25519_dalek::PublicKey::from(envelope.pubkey());
    let response_sym_key = derive_key(&client_public_key, &state.notify_keys.key_agreement_secret)?;
    let response_topic = sha256::digest(&response_sym_key);

    let msg: NotifyMessage<NotifyWatchSubscriptions> =
        decrypt_message(envelope, &response_sym_key)?;

    let id = msg.id;

    let request_auth =
        from_jwt::<WatchSubscriptionsRequestAuth>(&msg.params.watch_subscriptions_auth)?;

    if request_auth.act != "notify_watch_subscriptions" {
        return Err(AuthError::InvalidAct)?;
    }

    verify_identity(
        &request_auth.shared_claims.iss,
        &request_auth.ksu,
        &request_auth.sub,
    )
    .await?;

    // TODO verify `sub_auth.aud` matches `notify-server.identity_keypair`

    // TODO merge code with integration.rs#verify_jwt()
    //      - put desired `iss` value as an argument to make sure we verify it

    let identity: DecodedClientId =
        DecodedClientId(state.notify_keys.authentication_public.to_bytes());

    let now = Utc::now();
    let response_message = WatchSubscriptionsResponseAuth {
        shared_claims: SharedClaims {
            iat: now.timestamp() as u64,
            exp: add_ttl(now, NOTIFY_SUBSCRIBE_RESPONSE_TTL).timestamp() as u64,
            iss: format!("did:key:{identity}"),
        },
        aud: request_auth.shared_claims.iss,
        act: "notify_watch_subscriptions_response".to_string(),
        sbs: vec![], // TODO
    };
    let response_auth = sign_jwt(response_message, &state.notify_keys.authentication_secret)?;
    let response = NotifyResponse::<Value> {
        id,
        jsonrpc: "2.0".into(),
        result: json!({ "responseAuth": response_auth }), // TODO use structure
    };

    let envelope = Envelope::<EnvelopeType0>::new(&response_sym_key, response)?;
    let base64_notification = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    client
        .publish(
            response_topic.into(),
            base64_notification,
            NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_TAG,
            NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_TTL,
            false,
        )
        .await?;

    Ok(())
}
