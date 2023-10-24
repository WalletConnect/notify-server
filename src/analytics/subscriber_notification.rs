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
    pub notify_topic: Topic,
    pub notification_type: Arc<str>,
}

#[derive(Debug, Serialize, ParquetRecordWriter)]
pub struct SubscriberNotification {
    /// Time at which the event was generated
    pub event_at: chrono::NaiveDateTime,
    pub project_id: Arc<str>,
    /// Primary Key of the subscriber in the Notify Server database that the notificaiton is being sent to
    pub client_pk: String,
    pub account_hash: String,
    /// Relay message ID
    pub msg_id: Arc<str>,
    /// The topic that notifications are sent on
    pub notify_topic: Arc<str>,
    /// The notification type ID
    pub notification_type: Arc<str>,
}

impl From<SubscriberNotificationParams> for SubscriberNotification {
    fn from(params: SubscriberNotificationParams) -> Self {
        Self {
            project_id: params.project_id.into_value(),
            msg_id: params.msg_id,
            notify_topic: params.notify_topic.into_value(),
            client_pk: params.client_pk.to_string(),
            account_hash: sha256::digest(params.account.as_ref()),
            notification_type: params.notification_type,
            event_at: wc::analytics::time::now(),
        }
    }
}
