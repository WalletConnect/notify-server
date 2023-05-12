use {
    crate::types::{Notification, Subscribtion, Unsubscribe},
    serde::{Deserialize, Serialize},
};

pub type Topic = String;
/// Data structure representing PublishParams.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PublishParams {
    /// Topic to publish to.
    pub topic: Topic,
    /// Message to publish.
    pub message: String,
    /// Duration for which the message should be kept in the mailbox if it
    /// be delivered, in seconds.
    #[serde(rename = "ttl")]
    pub ttl_secs: u32,
    // #[serde(default, skip_serializing_if = "is_default")]
    /// A label that identifies what type of message is sent based on the RPC
    /// method used.
    pub tag: u32,
    /// A flag that identifies whether the server should trigger a
    /// webhook to a client through a push server.
    #[serde(default)]
    pub prompt: bool,
}

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
    #[serde(rename = "irn_publish", alias = "iridium_publish")]
    Publish(PublishParams),
    #[serde(rename = "wc_pushMessage")]
    Push(Notification),
    #[serde(rename = "irn_subscribe")]
    Subscribe(Subscribtion),
    #[serde(rename = "irn_unsubscribe")]
    Unsubscribe(Unsubscribe),
}
