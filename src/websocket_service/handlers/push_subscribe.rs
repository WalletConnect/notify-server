use {
    crate::{
        auth::SubscriptionAuth,
        handlers::subscribe_topic::ProjectData,
        log::info,
        state::AppState,
        types::{ClientData, Envelope, EnvelopeType0, EnvelopeType1},
        websocket_service::{
            derive_key,
            handlers::decrypt_message,
            NotifyMessage,
            NotifyResponse,
            NotifySubscribe,
        },
        wsclient::{new_rpc_request, WsClient},
        Result,
    },
    base64::Engine,
    mongodb::bson::doc,
    serde_json::{json, Value},
    std::sync::Arc,
    walletconnect_sdk::rpc::rpc::{Params, Payload, Publish, Subscription, SubscriptionData},
    x25519_dalek::{PublicKey, StaticSecret},
};

pub async fn handle(
    params: Subscription,
    state: &Arc<AppState>,
    client: &mut WsClient,
) -> Result<()> {
    let Subscription {
        data: SubscriptionData { message, topic, .. },
        ..
    } = params;

    let topic = topic.to_string();

    // Grab record from db
    let project_data = state
        .database
        .collection::<ProjectData>("project_data")
        .find_one(doc!("topic":topic.clone()), None)
        .await?
        .ok_or(crate::error::Error::NoProjectDataForTopic(topic))?;

    let envelope = Envelope::<EnvelopeType1>::from_bytes(
        base64::engine::general_purpose::STANDARD.decode(message.to_string())?,
    )?;

    let client_pubkey = envelope.pubkey();
    info!("pubkey: {}", hex::encode(client_pubkey));

    let response_sym_key = derive_key(hex::encode(client_pubkey), project_data.private_key)?;
    info!("response_sym_key: {}", &response_sym_key);

    let msg: NotifyMessage<NotifySubscribe> = decrypt_message(envelope, &response_sym_key)?;

    info!("msg: {:?}", &msg);

    let id = msg.id;

    let sub_auth = SubscriptionAuth::from_jwt(&msg.params.subscription_auth)?;

    let secret = StaticSecret::random_from_rng(chacha20poly1305::aead::OsRng {});
    let public = PublicKey::from(&secret);

    let response = NotifyResponse::<Value> {
        id,
        jsonrpc: "2.0".into(),
        result: json!({"publicKey": hex::encode(public.to_bytes())}),
    };
    info!("response: {}", serde_json::to_string(&response)?);

    let push_key = derive_key(hex::encode(client_pubkey), hex::encode(secret))?;
    info!("push_key: {}", &push_key);

    let envelope = Envelope::<EnvelopeType0>::new(&response_sym_key, response)?;

    let base64_notification = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    let key = hex::decode(response_sym_key)?;
    let response_topic = sha256::digest(&*key);
    info!("response_topic: {}", &response_topic);

    let request = new_rpc_request(Params::Publish(Publish {
        topic: response_topic.into(),
        message: base64_notification.into(),
        ttl_secs: 86400,
        tag: 4007,
        prompt: false,
    }));
    client.send_raw(Payload::Request(request)).await?;

    let client_data = ClientData {
        id: sub_auth.sub.trim_start_matches("did:pkh:").into(),
        relay_url: state.config.relay_url.clone(),
        sym_key: push_key.clone(),
        scope: sub_auth.scp.split(' ').map(|s| s.into()).collect(),
    };
    info!("Registering client: {:?}", &client_data);

    state
        .register_client(
            &project_data.id,
            &client_data,
            &url::Url::parse(&state.config.relay_url)?,
        )
        .await?;

    Ok(())
}
