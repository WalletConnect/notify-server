use {
    crate::model::types::AccountId,
    itertools::Itertools,
    parquet_derive::ParquetRecordWriter,
    relay_rpc::domain::{ProjectId, Topic},
    serde::Serialize,
    std::{
        collections::HashSet,
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
    pub project_pk: Uuid,
    pub project_id: ProjectId,
    pub pk: Uuid,
    pub account: AccountId,
    pub method: NotifyClientMethod,
    pub old_scope: HashSet<Arc<str>>,
    pub new_scope: HashSet<Arc<str>>,
    pub notification_topic: Topic,
    pub topic: Topic,
}

#[derive(Debug, Serialize, ParquetRecordWriter)]
pub struct SubscriberUpdate {
    /// Time at which the event was generated
    pub event_at: chrono::NaiveDateTime,
    /// Primary key of the project in the Notify Server database that the subscriber is subscribed to
    pub project_pk: Uuid,
    /// Project ID of the project that the subscriber is subscribed to
    pub project_id: Arc<str>,
    /// Primary Key of the subscriber in the Notify Server database
    pub pk: Uuid,
    /// Hash of the CAIP-10 account of the subscriber
    pub account_hash: String,
    /// The change that happend to the subscriber, can be subscribe, update, or unsubscribe
    pub method: String,
    /// Notification types that the subscriber was subscribed to before the update, separated by commas
    pub old_scope: String,
    /// Notification types that the subscriber is subscribed to after the update, separated by commas
    pub new_scope: String,
    /// The topic that notifications are sent on
    pub notification_topic: Arc<str>,
    /// The topic used to create or manage the subscription that the update message was published to
    pub topic: Arc<str>,
}

impl From<SubscriberUpdateParams> for SubscriberUpdate {
    fn from(client: SubscriberUpdateParams) -> Self {
        Self {
            event_at: wc::analytics::time::now(),
            project_pk: client.project_pk,
            project_id: client.project_id.into_value(),
            pk: client.pk,
            account_hash: sha256::digest(client.account.as_ref()),
            method: client.method.to_string(),
            old_scope: client.old_scope.iter().join(","),
            new_scope: client.new_scope.iter().join(","),
            notification_topic: client.notification_topic.into_value(),
            topic: client.topic.into_value(),
        }
    }
}
