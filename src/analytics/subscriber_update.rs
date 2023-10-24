use {
    crate::model::types::AccountId,
    parquet_derive::ParquetRecordWriter,
    relay_rpc::domain::{ProjectId, Topic},
    serde::Serialize,
    std::{
        fmt::{self, Display, Formatter},
        sync::Arc,
    },
    uuid::Uuid,
};

pub enum NotifyClientMethod {
    Subscribe,
    Update,
    Unsubscribe,
}

impl Display for NotifyClientMethod {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Subscribe => write!(f, "subscribe"),
            Self::Update => write!(f, "update"),
            Self::Unsubscribe => write!(f, "unsubscribe"),
        }
    }
}

pub struct SubscriberUpdateParams {
    pub project_id: ProjectId,
    pub pk: Uuid,
    pub account: AccountId,
    pub method: NotifyClientMethod,
    pub topic: Topic,
    pub notify_topic: Topic,
    pub old_scope: String,
    pub new_scope: String,
}

#[derive(Debug, Serialize, ParquetRecordWriter)]
pub struct SubscriberUpdate {
    pub event_at: chrono::NaiveDateTime,
    pub project_id: Arc<str>,
    pub pk: Uuid,
    pub account_hash: String,
    pub method: String, // subscribe, update, unsubscribe
    pub topic: Arc<str>,
    pub notify_topic: Arc<str>,
    pub old_scope: String,
    pub new_scope: String,
}

impl From<SubscriberUpdateParams> for SubscriberUpdate {
    fn from(client: SubscriberUpdateParams) -> Self {
        Self {
            pk: client.pk,
            method: client.method.to_string(),
            project_id: client.project_id.into_value(),
            account_hash: sha256::digest(client.account.as_ref()),
            topic: client.topic.into_value(),
            notify_topic: client.notify_topic.into_value(),
            old_scope: client.old_scope,
            new_scope: client.new_scope,
            event_at: wc::analytics::time::now(),
        }
    }
}