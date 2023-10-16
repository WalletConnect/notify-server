use {parquet_derive::ParquetRecordWriter, serde::Serialize, std::sync::Arc};

#[derive(Debug, Serialize, ParquetRecordWriter)]
#[serde(rename_all = "camelCase")]
pub struct NotifyMessage {
    pub project_id: Arc<str>,
    pub msg_id: Arc<str>,
    pub topic: Arc<str>,
    pub account: Arc<str>,
    pub account_hash: String,
    pub notification_type: Arc<str>,
    pub send_id: String,
    pub event_at: chrono::NaiveDateTime,
}
