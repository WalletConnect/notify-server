use serde::{Deserialize, Serialize};

pub mod health;
pub mod metrics;
pub mod notify;
pub mod register;

#[derive(Debug, Serialize, Deserialize)]
pub struct ClientData {
    #[serde(rename = "_id")]
    id: String,
    relay_url: String,
    sym_key: String,
}
