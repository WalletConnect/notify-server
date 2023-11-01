use {
    base64::Engine,
    notify_server::{
        auth::AuthError,
        handlers::notify_v0::JwtMessage,
        wsclient::{create_connection_opts, RelayClientEvent, RelayConnectionHandler},
    },
    rand::rngs::StdRng,
    rand_chacha::rand_core::OsRng,
    rand_core::SeedableRng,
    relay_client::websocket,
    relay_rpc::auth::ed25519_dalek::Keypair,
    std::sync::Arc,
    tokio::sync::mpsc::UnboundedReceiver,
    x25519_dalek::{PublicKey, StaticSecret},
};

pub const JWT_LEEWAY: i64 = 30;

pub async fn create_client(
    relay_url: &str,
    relay_project_id: &str,
    notify_url: &str,
) -> (
    Arc<relay_client::websocket::Client>,
    UnboundedReceiver<RelayClientEvent>,
) {
    let secret = StaticSecret::random_from_rng(OsRng);
    let _public = PublicKey::from(&secret);

    // Create a websocket client to communicate with relay
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    let connection_handler = RelayConnectionHandler::new("notify-client", tx);
    let wsclient = Arc::new(websocket::Client::new(connection_handler));

    let keypair = Keypair::generate(&mut StdRng::from_entropy());
    let opts = create_connection_opts(relay_url, relay_project_id, &keypair, notify_url).unwrap();
    wsclient.connect(&opts).await.unwrap();

    // Eat up the "connected" message
    _ = rx.recv().await.unwrap();

    (wsclient, rx)
}

// Workaround https://github.com/rust-lang/rust-clippy/issues/11613
#[allow(clippy::needless_return_with_question_mark)]
pub fn verify_jwt(jwt: &str, key: &str) -> notify_server::error::Result<JwtMessage> {
    // Refactor to call from_jwt() and then check `iss` with:
    // let pub_key = did_key.parse::<DecodedClientId>()?;
    // let key = jsonwebtoken::DecodingKey::from_ed_der(pub_key.as_ref());
    // Or perhaps do the opposite (i.e. serialize key into iss)

    let key = jsonwebtoken::DecodingKey::from_ed_der(&hex::decode(key).unwrap());

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
