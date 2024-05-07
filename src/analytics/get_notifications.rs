use {
    crate::model::types::AccountId,
    parquet_derive::ParquetRecordWriter,
    relay_rpc::domain::{ProjectId, Topic},
    serde::Serialize,
    std::sync::Arc,
    uuid::Uuid,
};

pub struct GetNotificationsParams {
    pub topic: Topic,
    pub message_id: Arc<str>,
    pub get_by_iss: Arc<str>,
    pub get_by_domain: Arc<str>,
    pub project_pk: Uuid,
    pub project_id: ProjectId,
    pub subscriber_pk: Uuid,
    pub subscriber_account: AccountId,
    pub notification_topic: Topic,
    pub subscriber_notification_pk: Uuid,
    pub notification_pk: Uuid,
    pub notification_type: Uuid,
    pub returned_count: usize,
}

#[derive(Debug, Serialize, ParquetRecordWriter)]
pub struct GetNotifications {
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
    /// The CAIP-10 account of the subscriber
    pub subscriber_account: String,
    /// Hash of the CAIP-10 account of the subscriber
    pub subscriber_account_hash: String,
    /// The topic that notifications are sent on
    pub notification_topic: Arc<str>,
    /// Primary key of the subscriber-specific notification in the Notify Server database
    pub subscriber_notification_id: String, // breaking change: rename to _pk
    /// Primary key of the notification in the Notify Server database
    pub notification_id: String, // breaking change: rename to _pk
    /// The notification type ID
    pub notification_type: String,
    /// The total number of notifications returned in the request
    pub returned_count: usize,
}

impl From<GetNotificationsParams> for GetNotifications {
    fn from(params: GetNotificationsParams) -> Self {
        Self {
            event_at: wc::analytics::time::now(),
            topic: params.topic.into_value(),
            message_id: params.message_id,
            get_by_iss: params.get_by_iss,
            get_by_domain: params.get_by_domain,
            project_pk: params.project_pk.to_string(),
            project_id: params.project_id.into_value(),
            subscriber_pk: params.subscriber_pk.to_string(),
            subscriber_account: params.subscriber_account.to_string(),
            subscriber_account_hash: sha256::digest(params.subscriber_account.as_ref()),
            notification_topic: params.notification_topic.into_value(),
            subscriber_notification_id: params.subscriber_notification_pk.to_string(),
            notification_id: params.notification_pk.to_string(),
            notification_type: params.notification_type.to_string(),
            returned_count: params.returned_count,
        }
    }
}
