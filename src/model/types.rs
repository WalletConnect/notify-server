use {
    chrono::{DateTime, Utc},
    relay_rpc::{
        domain::{ProjectId, Topic},
        new_type,
    },
    sqlx::FromRow,
    std::sync::Arc,
    uuid::Uuid,
};

// See /migrations/ERD.md

new_type!(
    #[doc = "Represents a CAIP-10 account ID."]
    #[as_ref(forward)]
    AccountId: Arc<str>
);

impl From<String> for AccountId {
    fn from(s: String) -> Self {
        Self(Arc::from(s.to_ascii_lowercase()))
    }
}

impl From<&str> for AccountId {
    fn from(s: &str) -> Self {
        Self(Arc::from(s.to_ascii_lowercase()))
    }
}

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
