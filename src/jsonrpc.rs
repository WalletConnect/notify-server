use serde::{Deserialize, Serialize};

// TODO: Move to using rust sdk

pub type Topic = String;
/// Data structure representing PublishParams.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PublishParams {
    /// Topic to publish to.
    pub topic: Topic,
    /// Message to publish.
    pub message: String,
    /// Duration for which the message should be kept in the mailbox if it can't
    /// be delivered, in seconds.
    #[serde(rename = "ttl")]
    pub ttl_secs: u32,
    // #[serde(default, skip_serializing_if = "is_default")]
    /// A label that identifies what type of message is sent based on the RPC
    /// method used.
    pub tag: u32,
    /// A flag that identifies whether the server should trigger a notification
    /// webhook to a client through a push server.
    #[serde(default)]
    pub prompt: bool,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct Notification {
    pub title: String,
    pub body: String,
    pub icon: String,
    pub url: String,
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

#[derive(Serialize, Deserialize, Debug)]
pub struct Subscribtion {
    pub topic: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Unsubscribe {
    pub topic: String,
    pub id: String,
}
