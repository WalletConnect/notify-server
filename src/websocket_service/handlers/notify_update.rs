use {
    crate::{
        auth::{
            from_jwt,
            sign_jwt,
            AuthError,
            SharedClaims,
            SubscriptionUpdateRequestAuth,
            SubscriptionUpdateResponseAuth,
        },
        handlers::subscribe_topic::ProjectData,
        state::AppState,
        types::{ClientData, Envelope, EnvelopeType0, LookupEntry},
        websocket_service::{
            handlers::{decrypt_message, notify_subscribe::RESPONSE_TTL},
            NotifyMessage,
            NotifyResponse,
            NotifyUpdate,
        },
        Result,
    },
    base64::Engine,
    mongodb::bson::doc,
    relay_rpc::domain::{ClientId, DecodedClientId},
    serde_json::{json, Value},
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
        .find_one(doc!("_id": lookup_data.account), None)
        .await?
        .ok_or(crate::error::Error::NoClientDataForTopic(topic.clone()))?;

    info!("[{request_id}] Fetched client: {:?}", &client_data);

    let envelope = Envelope::<EnvelopeType0>::from_bytes(
        base64::engine::general_purpose::STANDARD.decode(msg.message.to_string())?,
    )?;

    let msg: NotifyMessage<NotifyUpdate> = decrypt_message(envelope, &client_data.sym_key)?;

    let sub_auth = from_jwt::<SubscriptionUpdateRequestAuth>(&msg.params.update_auth)?;
    let sub_auth_hash = sha256::digest(msg.params.update_auth);

    if sub_auth.act != "notify_update" {
        return Err(AuthError::InvalidAct)?;
    }

    let response_sym_key = client_data.sym_key.clone();
    let client_data = ClientData {
        // TODO don't replace this, make sure it matches
        id: sub_auth.sub.trim_start_matches("did:pkh:").into(),
        relay_url: state.config.relay_url.clone(),
        sym_key: client_data.sym_key,
        scope: sub_auth.scp.split(' ').map(|s| s.into()).collect(),
        sub_auth_hash: sub_auth_hash.clone(),
        expiry: sub_auth.shared_claims.exp,
        ksu: sub_auth.shared_claims.ksu.clone(),
    };
    info!("[{request_id}] Updating client: {:?}", &client_data);

    state
        .register_client(
            &lookup_data.project_id,
            client_data,
            &url::Url::parse(&state.config.relay_url)?,
        )
        .await?;

    // response

    let decoded_client_id = DecodedClientId(
        hex::decode(project_data.identity_keypair.public_key.clone())?[0..32].try_into()?,
    );
    let identity = ClientId::from(decoded_client_id).to_string();

    let response_message = SubscriptionUpdateResponseAuth {
        shared_claims: SharedClaims {
            iat: chrono::Utc::now().timestamp() as u64,
            exp: (chrono::Utc::now() + chrono::Duration::seconds(RESPONSE_TTL as i64)).timestamp()
                as u64,
            iss: format!("did:key:{identity}"),
            ksu: sub_auth.shared_claims.ksu.clone(),
        },
        aud: sub_auth.shared_claims.iss,
        act: "notify_update_response".to_string(),
        sub: sub_auth_hash,
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

    let envelope = Envelope::<EnvelopeType0>::new(&response_sym_key, response)?;

    let base64_notification = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    let key = hex::decode(response_sym_key)?;
    let response_topic = sha256::digest(&*key);
    info!("[{request_id}] Response_topic: {}", &response_topic);

    client
        .publish(
            response_topic.into(),
            base64_notification,
            4009,
            Duration::from_secs(RESPONSE_TTL),
            false,
        )
        .await?;

    Ok(())
}
