use {
    crate::model::types::AccountId,
    parquet_derive::ParquetRecordWriter,
    relay_rpc::domain::{ProjectId, Topic},
    serde::Serialize,
    std::sync::Arc,
    uuid::Uuid,
};

pub struct FollowNotificationLinkParams {
    pub project_pk: Uuid,
    pub project_id: ProjectId,
    pub subscriber_pk: Uuid,
    pub subscriber_account: AccountId,
    pub notification_topic: Topic,
    pub subscriber_notification_id: Uuid,
    pub notification_id: Uuid,
    pub notification_type: Uuid,
}

#[derive(Debug, Serialize, ParquetRecordWriter)]
pub struct FollowNotificationLink {
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
    /// The ID of the subscriber-specific notification
    pub subscriber_notification_id: String,
    /// The ID of the notification
    pub notification_id: String,
    /// The notification type ID
    pub notification_type: String,
}

impl From<FollowNotificationLinkParams> for FollowNotificationLink {
    fn from(params: FollowNotificationLinkParams) -> Self {
        Self {
            project_pk: params.project_pk.to_string(),
            project_id: params.project_id.into_value(),
            subscriber_pk: params.subscriber_pk.to_string(),
            subscriber_account_hash: sha256::digest(params.subscriber_account.as_ref()),
            notification_topic: params.notification_topic.into_value(),
            subscriber_notification_id: params.subscriber_notification_id.to_string(),
            notification_id: params.notification_id.to_string(),
            notification_type: params.notification_type.to_string(),
        }
    }
}
