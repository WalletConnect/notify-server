use {parquet_derive::ParquetRecordWriter, serde::Serialize, std::sync::Arc};

pub struct NotifyMessageParams {
    pub project_id: Arc<str>,
    pub msg_id: Arc<str>,
    pub topic: Arc<str>,
    pub account: Arc<str>,
    pub notification_type: Arc<str>,
    pub send_id: String,
}

#[derive(Debug, Serialize, ParquetRecordWriter)]
pub struct NotifyMessage {
    pub project_id: Arc<str>,
    pub msg_id: Arc<str>,
    pub topic: Arc<str>,
    pub account_hash: String,
    pub notification_type: Arc<str>,
    pub send_id: String,
    pub event_at: chrono::NaiveDateTime,
}

impl From<NotifyMessageParams> for NotifyMessage {
    fn from(message: NotifyMessageParams) -> Self {
        Self {
            project_id: message.project_id,
            msg_id: message.msg_id,
            topic: message.topic,
            account_hash: sha256::digest(message.account.as_ref()),
            notification_type: message.notification_type,
            send_id: message.send_id,
            event_at: wc::analytics::time::now(),
        }
    }
}
