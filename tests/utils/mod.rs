use {
    base64::Engine,
    ed25519_dalek::{Signer, VerifyingKey},
    k256::ecdsa::SigningKey,
    notify_server::{
        auth::{AuthError, GetSharedClaims, SharedClaims},
        model::types::AccountId,
        notify_message::JwtMessage,
        relay_client_helpers::create_ws_connect_options,
        services::websocket_server::relay_ws_client::{RelayClientEvent, RelayConnectionHandler},
    },
    rand::rngs::StdRng,
    rand_chacha::rand_core::OsRng,
    rand_core::SeedableRng,
    relay_client::websocket,
    relay_rpc::{
        auth::ed25519_dalek::Keypair,
        domain::ProjectId,
        jwt::{JwtHeader, JWT_HEADER_ALG, JWT_HEADER_TYP},
    },
    serde::Serialize,
    sha2::Digest,
    sha3::Keccak256,
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
    let relay_ws_client = Arc::new(websocket::Client::new(connection_handler));

    let keypair = Keypair::generate(&mut StdRng::from_entropy());
    let opts =
        create_ws_connect_options(&keypair, relay_url, notify_url, relay_project_id).unwrap();
    relay_ws_client.connect(&opts).await.unwrap();

    // Eat up the "connected" message
    _ = rx.recv().await.unwrap();

    (relay_ws_client, rx)
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

pub fn generate_account() -> (SigningKey, AccountId) {
    let account_signing_key = k256::ecdsa::SigningKey::random(&mut OsRng);
    let address = &Keccak256::default()
        .chain_update(
            &account_signing_key
                .verifying_key()
                .to_encoded_point(false)
                .as_bytes()[1..],
        )
        .finalize()[12..];
    let account = format!("eip155:1:0x{}", hex::encode(address)).into();
    (account_signing_key, account)
}

pub fn encode_auth<T: Serialize>(auth: &T, signing_key: &ed25519_dalek::SigningKey) -> String {
    let data = JwtHeader {
        typ: JWT_HEADER_TYP,
        alg: JWT_HEADER_ALG,
    };
    let header = serde_json::to_string(&data).unwrap();
    let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(header);

    let claims = {
        let json = serde_json::to_string(auth).unwrap();
        base64::engine::general_purpose::STANDARD_NO_PAD.encode(json)
    };

    let message = format!("{header}.{claims}");

    let signature = signing_key.sign(message.as_bytes());
    let signature = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature.to_bytes());

    format!("{message}.{signature}")
}

#[derive(Debug, Clone, Serialize)]
pub struct UnregisterIdentityRequestAuth {
    #[serde(flatten)]
    pub shared_claims: SharedClaims,
    /// corresponding blockchain account (did:pkh)
    pub pkh: String,
}

impl GetSharedClaims for UnregisterIdentityRequestAuth {
    fn get_shared_claims(&self) -> &SharedClaims {
        &self.shared_claims
    }
}
