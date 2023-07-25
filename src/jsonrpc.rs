use {
    crate::types::Notification,
    serde::{Deserialize, Serialize},
};

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcPayload {
    pub id: u64,
    pub jsonrpc: String,
    #[serde(flatten)]
    pub params: JsonRpcParams,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum JsonRpcParams {
    #[serde(rename = "wc_pushMessage")]
    Push(Notification),
}
