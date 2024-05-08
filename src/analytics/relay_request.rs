use {
    crate::services::public_http_server::handlers::relay_webhook::RelayIncomingMessage,
    chrono::{DateTime, NaiveDateTime, Utc},
    parquet_derive::ParquetRecordWriter,
    relay_rpc::domain::Topic,
    serde::Serialize,
    std::sync::Arc,
};

pub struct RelayResponseParams {
    pub request: Arc<RelayIncomingMessage>,

    pub response_message_id: Arc<str>,
    pub response_topic: Topic,
    pub response_tag: u32,
    pub response_initiated_at: DateTime<Utc>,
    pub response_finished_at: DateTime<Utc>,
    pub response_success: bool,
}

#[derive(Debug, Serialize, ParquetRecordWriter)]
pub struct RelayRequest {
    /// Time at which the event was generated
    pub event_at: NaiveDateTime,

    /// Relay message ID of request
    pub request_message_id: Arc<str>,
    /// Relay topic of request
    pub request_topic: Arc<str>,
    /// Relay tag of request
    pub request_tag: u32,
    /// Time at which the request was received
    pub request_received_at: NaiveDateTime,

    /// Relay message ID of response
    pub response_message_id: Arc<str>,
    /// Relay topic of response
    pub response_topic: Arc<str>,
    /// Relay tag of response
    pub response_tag: u32,
    /// Time at which the publish request was initiated
    pub response_initiated_at: NaiveDateTime,
    /// Time at which the publish request stopped
    pub response_finished_at: NaiveDateTime,
    /// If the publish request was ultimatly successful or not
    pub response_success: bool,
}

impl From<RelayResponseParams> for RelayRequest {
    fn from(params: RelayResponseParams) -> Self {
        Self {
            event_at: wc::analytics::time::now(),

            request_message_id: params.request.message_id.clone(),
            request_topic: params.request.topic.value().clone(),
            request_tag: params.request.tag,
            request_received_at: params.request.received_at.naive_utc(),

            response_message_id: params.response_message_id.clone(),
            response_topic: params.response_topic.value().clone(),
            response_tag: params.response_tag,
            response_initiated_at: params.response_initiated_at.naive_utc(),
            response_finished_at: params.response_finished_at.naive_utc(),
            response_success: params.response_success,
        }
    }
}
