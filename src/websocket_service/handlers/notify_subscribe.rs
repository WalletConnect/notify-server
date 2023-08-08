use {
    crate::{
        auth::SubscriptionAuth,
        handlers::subscribe_topic::ProjectData,
        state::AppState,
        types::{ClientData, Envelope, EnvelopeType0, EnvelopeType1},
        websocket_service::{
            derive_key,
            handlers::decrypt_message,
            NotifyMessage,
            NotifyResponse,
            NotifySubscribe,
        },
        Result,
    },
    base64::Engine,
    mongodb::bson::doc,
    serde_json::{json, Value},
    std::{sync::Arc, time::Duration},
    tracing::info,
    x25519_dalek::{PublicKey, StaticSecret},
};

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
    info!("[{request_id}] User pubkey: {}", hex::encode(client_pubkey));

    let response_sym_key = derive_key(
        hex::encode(client_pubkey),
        project_data.signing_keypair.private_key,
    )?;
    info!("[{request_id}] Response_sym_key: {}", &response_sym_key);

    let msg: NotifyMessage<NotifySubscribe> = decrypt_message(envelope, &response_sym_key)?;

    info!("[{request_id}] Register message: {:?}", &msg);

    let id = msg.id;

    let sub_auth = SubscriptionAuth::from_jwt(&msg.params.subscription_auth)?;
    let sub_auth_hash = sha256::digest(msg.params.subscription_auth);

    // TODO verify `sub_auth.aud` matches `project_data.identity_keypair` (?)
    // TODO verify `sub_auth.app` matches `project_data.dapp_url`

    let secret = StaticSecret::random_from_rng(chacha20poly1305::aead::OsRng {});
    let public = PublicKey::from(&secret);

    let response = NotifyResponse::<Value> {
        id,
        jsonrpc: "2.0".into(),
        result: json!({"publicKey": hex::encode(public.to_bytes())}), // TODO change to signed JWT
    };
    info!(
        "[{request_id}] Response for user: {}",
        serde_json::to_string(&response)?
    );

    let push_key = derive_key(hex::encode(client_pubkey), hex::encode(secret))?;
    info!("[{request_id}] Derived push_key: {}", &push_key);

    let envelope = Envelope::<EnvelopeType0>::new(&response_sym_key, response)?;

    let base64_notification = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    let key = hex::decode(response_sym_key)?;
    let response_topic = sha256::digest(&*key);
    info!("[{request_id}] Response_topic: {}", &response_topic);

    let client_data = ClientData {
        id: sub_auth.sub.trim_start_matches("did:pkh:").into(),
        relay_url: state.config.relay_url.clone(),
        sym_key: push_key.clone(),
        scope: sub_auth.scp.split(' ').map(|s| s.into()).collect(),
        expiry: sub_auth.exp,
        sub_auth_hash,
        ksu: sub_auth.ksu,
    };

    let push_topic = sha256::digest(&*hex::decode(&push_key)?);

    // This noop message is making relay aware that this topics TTL should be
    // extended
    info!(
        "[{request_id}] Sending settle message on topic {}",
        &push_topic
    );

    // Registers account and subscribes to topic
    info!(
        "[{request_id}] Registering account: {:?} with topic: {} at project: {}. Scope: {:?}",
        &client_data.id, &push_topic, &project_data.id, &client_data.scope
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
            push_topic.clone().into(),
            "",
            4050,
            Duration::from_secs(300),
            false,
        )
        .await?;

    info!(
        "[{request_id}] Settle message sent on topic {}",
        &push_topic
    );

    client
        .publish(
            response_topic.into(),
            base64_notification,
            4001,
            Duration::from_secs(86400),
            false,
        )
        .await?;

    Ok(())
}
