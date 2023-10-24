use {
    crate::model::types::AccountId,
    parquet_derive::ParquetRecordWriter,
    relay_rpc::domain::{ProjectId, Topic},
    serde::Serialize,
    std::sync::Arc,
    uuid::Uuid,
};

pub struct SubscriberNotificationParams {
    pub project_id: ProjectId,
    pub client_pk: Uuid,
    pub account: AccountId,
    pub msg_id: Arc<str>,
    pub topic: Topic,
    pub notification_type: Arc<str>,
}

#[derive(Debug, Serialize, ParquetRecordWriter)]
pub struct SubscriberNotification {
    pub event_at: chrono::NaiveDateTime,
    pub project_id: Arc<str>,
    pub client_pk: String,
    pub account_hash: String,
    pub msg_id: Arc<str>,
    pub topic: Arc<str>,
    pub notification_type: Arc<str>,
}

impl From<SubscriberNotificationParams> for SubscriberNotification {
    fn from(message: SubscriberNotificationParams) -> Self {
        Self {
            project_id: message.project_id.into_value(),
            msg_id: message.msg_id,
            topic: message.topic.into_value(),
            client_pk: message.client_pk.to_string(),
            account_hash: sha256::digest(message.account.as_ref()),
            notification_type: message.notification_type,
            event_at: wc::analytics::time::now(),
        }
    }
}
