use {
    base64::Engine,
    ed25519_dalek::VerifyingKey,
    notify_server::{
        auth::AuthError,
        relay_client_helpers::create_ws_connect_options,
        services::{
            public_http::handlers::notify_v0::JwtMessage,
            websocket_service::wsclient::{RelayClientEvent, RelayConnectionHandler},
        },
    },
    rand::rngs::StdRng,
    rand_core::SeedableRng,
    relay_client::websocket,
    relay_rpc::{auth::ed25519_dalek::Keypair, domain::ProjectId},
    std::sync::Arc,
    tokio::sync::mpsc::UnboundedReceiver,
    url::Url,
};

pub const JWT_LEEWAY: i64 = 30;

pub async fn create_client(
    relay_url: Url,
    relay_project_id: ProjectId,
    notify_url: Url,
) -> (
    Arc<relay_client::websocket::Client>,
    UnboundedReceiver<RelayClientEvent>,
) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let connection_handler = RelayConnectionHandler::new("notify-client", tx);
    let wsclient = Arc::new(websocket::Client::new(connection_handler));

    let keypair = Keypair::generate(&mut StdRng::from_entropy());
    let opts =
        create_ws_connect_options(&keypair, relay_url, notify_url, relay_project_id).unwrap();
    wsclient.connect(&opts).await.unwrap();

    // Eat up the "connected" message
    _ = rx.recv().await.unwrap();

    (wsclient, rx)
}

// Workaround https://github.com/rust-lang/rust-clippy/issues/11613
#[allow(clippy::needless_return_with_question_mark)]
pub fn verify_jwt(jwt: &str, key: &VerifyingKey) -> notify_server::error::Result<JwtMessage> {
    // Refactor to call from_jwt() and then check `iss` with:
    // let pub_key = did_key.parse::<DecodedClientId>()?;
    // let key = jsonwebtoken::DecodingKey::from_ed_der(pub_key.as_ref());
    // Or perhaps do the opposite (i.e. serialize key into iss)

    let key = jsonwebtoken::DecodingKey::from_ed_der(key.as_bytes());

    let mut parts = jwt.rsplitn(2, '.');

    let (Some(signature), Some(message)) = (parts.next(), parts.next()) else {
        return Err(AuthError::Format)?;
    };

    // Finally, verify signature.
    let sig_result = jsonwebtoken::crypto::verify(
        signature,
        message.as_bytes(),
        &key,
        jsonwebtoken::Algorithm::EdDSA,
    );

    match sig_result {
        Ok(true) => Ok(serde_json::from_slice::<JwtMessage>(
            &base64::engine::general_purpose::STANDARD_NO_PAD
                .decode(jwt.split('.').nth(1).unwrap())
                .unwrap(),
        )?),
        Ok(false) | Err(_) => Err(AuthError::InvalidSignature)?,
    }
}
