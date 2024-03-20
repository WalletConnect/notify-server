use {
    crate::model::types::AccountId,
    parquet_derive::ParquetRecordWriter,
    relay_rpc::domain::{ProjectId, Topic},
    serde::Serialize,
    std::sync::Arc,
    uuid::Uuid,
};

pub struct MarkNotificationsAsReadParams {
    pub topic: Topic,
    pub message_id: Arc<str>,
    pub by_iss: Arc<str>,
    pub by_domain: Arc<str>,
    pub project_pk: Uuid,
    pub project_id: ProjectId,
    pub subscriber_pk: Uuid,
    pub subscriber_account: AccountId,
    pub notification_topic: Topic,
    pub subscriber_notification_pk: Uuid,
    pub notification_pk: Uuid,
    pub marked_count: usize,
}

#[derive(Debug, Serialize, ParquetRecordWriter)]
pub struct MarkNotificationsAsRead {
    /// Time at which the event was generated
    pub event_at: chrono::NaiveDateTime,
    /// The relay topic used to manage the subscription that the get notifications request message was published to
    pub topic: Arc<str>,
    /// Relay message ID of request
    pub message_id: Arc<str>,
    /// JWT iss that made the request
    pub get_by_iss: Arc<str>,
    /// CACAO domain that made the request
    pub get_by_domain: Arc<str>,
    /// Primary key of the project in the Notify Server database that the subscriber is subscribed to
    pub project_pk: String,
    /// Project ID of the project that the subscriber is subscribed to
    pub project_id: Arc<str>,
    /// Primary Key of the subscriber in the Notify Server database
    pub subscriber_pk: String,
    /// Hash of the CAIP-10 account of the subscriber
    pub subscriber_account_hash: String,
    /// The topic that notifications are sent on
    pub notification_topic: Arc<str>,
    /// Primary key of the subscriber-specific notification in the Notify Server database
    pub subscriber_notification_pk: String,
    /// Primary key of the notification in the Notify Server database
    pub notification_pk: String,
    /// The total number of notifications returned in the request
    pub marked_count: usize,
}

impl From<MarkNotificationsAsReadParams> for MarkNotificationsAsRead {
    fn from(params: MarkNotificationsAsReadParams) -> Self {
        Self {
            event_at: wc::analytics::time::now(),
            topic: params.topic.into_value(),
            message_id: params.message_id,
            get_by_iss: params.by_iss,
            get_by_domain: params.by_domain,
            project_pk: params.project_pk.to_string(),
            project_id: params.project_id.into_value(),
            subscriber_pk: params.subscriber_pk.to_string(),
            subscriber_account_hash: sha256::digest(params.subscriber_account.as_ref()),
            notification_topic: params.notification_topic.into_value(),
            subscriber_notification_pk: params.subscriber_notification_pk.to_string(),
            notification_pk: params.notification_pk.to_string(),
            marked_count: params.marked_count,
        }
    }
}
