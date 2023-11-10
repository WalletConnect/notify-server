use {
    crate::{auth::add_ttl, error::Result, model::types::AccountId, spec::NOTIFY_MESSAGE_TTL},
    base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine},
    chrono::Utc,
    ed25519_dalek::{Signer, SigningKey},
    relay_rpc::{
        domain::ClientId,
        jwt::{JwtHeader, JWT_HEADER_ALG, JWT_HEADER_TYP},
    },
    serde::{Deserialize, Serialize},
    std::sync::Arc,
    uuid::Uuid,
};

pub struct ProjectSigningDetails {
    pub identity: ClientId,
    pub private_key: SigningKey,
    pub app: Arc<str>,
}

pub fn sign_message(
    msg: Arc<JwtNotification>,
    account: AccountId,
    ProjectSigningDetails {
        identity,
        private_key,
        app,
    }: &ProjectSigningDetails,
) -> Result<String> {
    let now = Utc::now();
    let message = URL_SAFE_NO_PAD.encode(serde_json::to_string(&JwtMessage {
        iat: now.timestamp(),
        exp: add_ttl(now, NOTIFY_MESSAGE_TTL).timestamp(),
        iss: format!("did:key:{identity}"),
        act: "notify_message".to_string(),
        sub: format!("did:pkh:{account}"),
        app: app.clone(),
        msg,
    })?);

    let header = URL_SAFE_NO_PAD.encode(serde_json::to_string(&JwtHeader {
        typ: JWT_HEADER_TYP,
        alg: JWT_HEADER_ALG,
    })?);

    let message = format!("{header}.{message}");
    let signature = private_key.sign(message.as_bytes());
    let signature = URL_SAFE_NO_PAD.encode(signature.to_bytes());

    Ok(format!("{message}.{signature}"))
}

#[derive(Serialize, Deserialize, Debug)]
pub struct JwtMessage {
    pub iat: i64, // issued at
    pub exp: i64, // expiry
    // TODO: This was changed from notify pubkey, should be confirmed if we want to keep this
    pub iss: String,               // dapps identity key
    pub act: String,               // action intent (must be "notify_message")
    pub sub: String,               // did:pkh of blockchain account
    pub app: Arc<str>,             // dapp domain url
    pub msg: Arc<JwtNotification>, // message
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct JwtNotification {
    pub r#type: Uuid,
    pub title: String,
    pub body: String,
    pub icon: String,
    pub url: String,
}
