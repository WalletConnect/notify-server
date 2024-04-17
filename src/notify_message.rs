use {
    crate::{
        auth::{add_ttl, sign_jwt, DidWeb, SignJwtError},
        model::types::AccountId,
        spec::{NOTIFY_MESSAGE_ACT, NOTIFY_MESSAGE_TTL},
    },
    chrono::Utc,
    relay_rpc::{auth::ed25519_dalek::SigningKey, domain::DecodedClientId},
    serde::{Deserialize, Serialize},
    sqlx::prelude::FromRow,
    std::sync::Arc,
    uuid::Uuid,
};

pub struct ProjectSigningDetails {
    pub decoded_client_id: DecodedClientId,
    pub private_key: SigningKey,
    pub app: DidWeb,
}

pub fn sign_message(
    msg: Arc<JwtNotification>,
    account: AccountId,
    ProjectSigningDetails {
        decoded_client_id,
        private_key,
        app,
    }: &ProjectSigningDetails,
) -> Result<String, SignJwtError> {
    let now = Utc::now();
    let message = NotifyMessage {
        iat: now.timestamp(),
        exp: add_ttl(now, NOTIFY_MESSAGE_TTL).timestamp(),
        iss: decoded_client_id.to_did_key(),
        // no `aud` because any client can receive this message
        act: NOTIFY_MESSAGE_ACT.to_owned(),
        sub: account.to_did_pkh(),
        app: app.clone(),
        msg,
    };

    sign_jwt(message, private_key)
}

#[derive(Serialize, Deserialize, Debug)]
pub struct NotifyMessage {
    pub iat: i64, // issued at
    pub exp: i64, // expiry
    // TODO: This was changed from notify pubkey, should be confirmed if we want to keep this
    pub iss: String,               // dapps identity key
    pub act: String,               // action intent (must be "notify_message")
    pub sub: String,               // did:pkh of blockchain account
    pub app: DidWeb,               // dapp domain url
    pub msg: Arc<JwtNotification>, // message
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone, FromRow)]
pub struct JwtNotification {
    pub id: Uuid,
    pub sent_at: i64,
    pub r#type: Uuid,
    pub title: String,
    pub body: String,
    pub icon: String,
    pub url: String,
    pub data: Option<String>,
    pub is_read: bool,
}
