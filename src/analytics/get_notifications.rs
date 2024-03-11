use {
    // crate::model::types::AccountId,
    // itertools::Itertools,
    parquet_derive::ParquetRecordWriter,
    // relay_rpc::domain::{ProjectId, Topic},
    serde::Serialize,
    // std::{
    //     collections::HashSet,
    //     fmt::{self, Display, Formatter},
    //     sync::Arc,
    // },
    // uuid::Uuid,
};

pub struct GetNotificationsParams {
    // pub project_pk: Uuid,
    // pub project_id: ProjectId,
    // pub pk: Uuid,
    // pub account: AccountId,
    // pub updated_by_iss: Arc<str>,
    // pub updated_by_domain: String,
    // pub method: NotifyClientMethod,
    // pub old_scope: HashSet<Uuid>,
    // pub new_scope: HashSet<Uuid>,
    // pub notification_topic: Topic,
    // pub topic: Topic,
}

#[derive(Debug, Serialize, ParquetRecordWriter)]
pub struct GetNotifications {
    // /// Time at which the event was generated
    // pub event_at: chrono::NaiveDateTime,
    // /// Primary key of the project in the Notify Server database that the subscriber is subscribed to
    // pub project_pk: String,
    // /// Project ID of the project that the subscriber is subscribed to
    // pub project_id: Arc<str>,
    // /// Primary Key of the subscriber in the Notify Server database
    // pub pk: String,
    // /// Hash of the CAIP-10 account of the subscriber
    // pub account_hash: String,
    // /// JWT iss that made the update
    // pub updated_by_iss: Arc<str>,
    // /// CACAO domain that made the update
    // pub updated_by_domain: String,
    // /// The change that happend to the subscriber, can be subscribe, update, or unsubscribe
    // pub method: String,
    // /// Notification types that the subscriber was subscribed to before the update, separated by commas
    // pub old_scope: String,
    // /// Notification types that the subscriber is subscribed to after the update, separated by commas
    // pub new_scope: String,
    // /// The topic that notifications are sent on
    // pub notification_topic: Arc<str>,
    // /// The topic used to create or manage the subscription that the update message was published to
    // pub topic: Arc<str>,
}

impl From<GetNotificationsParams> for GetNotifications {
    fn from(_params: GetNotificationsParams) -> Self {
        Self {
            // event_at: wc::analytics::time::now(),
            // project_pk: params.project_pk.to_string(),
            // project_id: params.project_id.into_value(),
            // pk: params.pk.to_string(),
            // account_hash: sha256::digest(params.account.as_ref()),
            // updated_by_iss: params.updated_by_iss,
            // updated_by_domain: params.updated_by_domain,
            // method: params.method.to_string(),
            // old_scope: params.old_scope.iter().join(","),
            // new_scope: params.new_scope.iter().join(","),
            // notification_topic: params.notification_topic.into_value(),
            // topic: params.topic.into_value(),
        }
    }
}
