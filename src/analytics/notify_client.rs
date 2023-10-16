use {parquet_derive::ParquetRecordWriter, serde::Serialize};

#[derive(Debug, Serialize, ParquetRecordWriter)]
#[serde(rename_all = "camelCase")]
pub struct NotifyClient {
    pub pk: String,
    pub method: String, // subscribe, update, unsubscribe
    pub project_id: String,
    pub account: String,
    pub account_hash: String,
    pub topic: String,
    pub notify_topic: String,
    pub old_scope: String,
    pub new_scope: String,
    pub event_at: chrono::NaiveDateTime,
}
