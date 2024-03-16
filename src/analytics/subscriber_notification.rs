use {
    crate::model::types::AccountId,
    parquet_derive::ParquetRecordWriter,
    relay_rpc::domain::{ProjectId, Topic},
    serde::Serialize,
    std::sync::Arc,
    uuid::Uuid,
};

pub struct SubscriberNotificationParams {
    pub project_pk: Uuid,
    pub project_id: ProjectId,
    pub subscriber_pk: Uuid,
    pub account: AccountId,
    pub subscriber_notification_pk: Uuid,
    pub notification_pk: Uuid,
    pub notification_type: Uuid,
    pub notify_topic: Topic,
    pub message_id: Arc<str>,
}

#[derive(Debug, Serialize, ParquetRecordWriter)]
pub struct SubscriberNotification {
    /// Time at which the event was generated
    pub event_at: chrono::NaiveDateTime,
    /// Primary key of the project in the Notify Server database that the notification was sent from and the subscriber is subscribed to
    pub project_pk: String,
    /// Project ID of the project that the notification was sent from and the subscriber is subscribed to
    pub project_id: Arc<str>,
    /// Primary key of the subscriber in the Notify Server database that the notificaiton is being sent to
    pub subscriber_pk: String,
    /// Hash of the CAIP-10 account of the subscriber
    pub account_hash: String,
    /// The ID of the subscriber-specific notification
    pub subscriber_notification_pk: String,
    /// The ID of the notification
    pub notification_pk: String,
    /// The notification type ID
    pub notification_type: String,
    /// The topic that the notification was sent on
    pub notification_topic: Arc<str>,
    /// Relay message ID of the notification
    pub message_id: Arc<str>,
}

impl From<SubscriberNotificationParams> for SubscriberNotification {
    fn from(params: SubscriberNotificationParams) -> Self {
        Self {
            event_at: wc::analytics::time::now(),
            project_pk: params.project_pk.to_string(),
            project_id: params.project_id.into_value(),
            subscriber_pk: params.subscriber_pk.to_string(),
            account_hash: sha256::digest(params.account.as_ref()),
            subscriber_notification_pk: params.subscriber_notification_pk.to_string(),
            notification_pk: params.notification_pk.to_string(),
            notification_type: params.notification_type.to_string(),
            notification_topic: params.notify_topic.into_value(),
            message_id: params.message_id,
        }
    }
}
