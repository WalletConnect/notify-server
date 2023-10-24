use {
    parquet_derive::ParquetRecordWriter,
    serde::Serialize,
    std::fmt::{self, Display, Formatter},
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

pub struct NotifyClientParams {
    pub pk: String,
    pub method: NotifyClientMethod,
    pub project_id: String,
    pub account: String,
    pub topic: String,
    pub notify_topic: String,
    pub old_scope: String,
    pub new_scope: String,
}

#[derive(Debug, Serialize, ParquetRecordWriter)]
pub struct NotifyClient {
    pub pk: String,
    pub method: String, // subscribe, update, unsubscribe
    pub project_id: String,
    pub account_hash: String,
    pub topic: String,
    pub notify_topic: String,
    pub old_scope: String,
    pub new_scope: String,
    pub event_at: chrono::NaiveDateTime,
}

impl From<NotifyClientParams> for NotifyClient {
    fn from(client: NotifyClientParams) -> Self {
        Self {
            pk: client.pk,
            method: client.method.to_string(),
            project_id: client.project_id,
            account_hash: sha256::digest(client.account.as_bytes()),
            topic: client.topic,
            notify_topic: client.notify_topic,
            old_scope: client.old_scope,
            new_scope: client.new_scope,
            event_at: wc::analytics::time::now(),
        }
    }
}
