use {parquet_derive::ParquetRecordWriter, serde::Serialize, std::sync::Arc};

#[derive(Debug, Serialize, ParquetRecordWriter)]
#[serde(rename_all = "camelCase")]
pub struct MessageInfo {
    pub region: Option<Arc<str>>,
    pub country: Option<Arc<str>>,
    pub continent: Option<Arc<str>>,
    pub project_id: Arc<str>,
    pub msg_id: Arc<str>,
    pub topic: Arc<str>,
    pub account: Arc<str>,
    pub sent_at: chrono::NaiveDateTime,
}
