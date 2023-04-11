use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RegisterBody {
    pub account: String,
    #[serde(default = "default_relay_url")]
    pub relay_url: String,
    pub sym_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClientData {
    #[serde(rename = "_id")]
    pub id: String,
    pub relay_url: String,
    pub sym_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LookupEntry {
    #[serde(rename = "_id")]
    pub topic: String,
    pub project_id: String,
    pub account: String,
}

// TODO: Load this from env
fn default_relay_url() -> String {
    "wss://relay.walletconnect.com".to_string()
}
