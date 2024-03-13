use {
    crate::{
        rpc::{decode_key, DecodeKeyError},
        utils::get_client_id,
    },
    chrono::{DateTime, Utc},
    relay_rpc::{
        auth::ed25519_dalek::VerifyingKey,
        domain::{DecodedClientId, ProjectId, Topic},
    },
    sqlx::FromRow,
    thiserror::Error,
    uuid::Uuid,
};

// See /migrations/ERD.md

mod account_id;
pub use account_id::*;

#[derive(Debug, FromRow)]
pub struct Project {
    pub id: Uuid,
    #[sqlx(try_from = "String")]
    pub project_id: ProjectId,
    pub app_domain: String,
    #[sqlx(try_from = "String")]
    pub topic: Topic,
    pub authentication_public_key: String,
    pub authentication_private_key: String,
    pub subscribe_public_key: String,
    pub subscribe_private_key: String,
}

#[derive(Debug, Error)]
pub enum GetAuthenticationClientIdError {
    #[error("Decode key: {0}")]
    DecodeKey(#[from] DecodeKeyError),

    #[error("Parse verifying key: {0}")]
    ParseVerifyingKey(#[from] k256::ecdsa::Error),
}

impl Project {
    pub fn get_authentication_client_id(
        &self,
    ) -> Result<DecodedClientId, GetAuthenticationClientIdError> {
        Ok(get_client_id(&VerifyingKey::from_bytes(&decode_key(
            &self.authentication_public_key,
        )?)?))
    }
}

#[derive(Debug, FromRow)]
pub struct Subscriber {
    pub id: Uuid,
    pub project: Uuid,
    /// CAIP-10 account
    #[sqlx(try_from = "String")]
    pub account: AccountId,
    pub sym_key: String,
    #[sqlx(try_from = "String")]
    pub topic: Topic,
    pub expiry: DateTime<Utc>,
}

#[derive(Debug)]
pub struct SubscriptionWatcher {
    pub account: AccountId,
    /// Project the watcher is authorized for. None for all.
    pub project: Option<Uuid>,
    pub did_key: String,
    pub sym_key: String,
    pub expiry: DateTime<Utc>,
}
