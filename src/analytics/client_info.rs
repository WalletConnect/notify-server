use {parquet_derive::ParquetRecordWriter, serde::Serialize, std::sync::Arc};

#[derive(Debug, Serialize, ParquetRecordWriter)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    pub project_id: Arc<str>,
    pub account: Arc<str>,
    pub topic: Arc<str>,
    pub registered_at: chrono::NaiveDateTime,
}

impl ClientInfo {
    pub fn new(
        project_id: Arc<str>,
        account: Arc<str>,
        topic: Arc<str>,
        registered_at: chrono::NaiveDateTime,
    ) -> Self {
        Self {
            project_id,
            account,
            topic,
            registered_at,
        }
    }
}
