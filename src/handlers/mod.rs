use serde::{Deserialize, Serialize};

pub mod health;
pub mod notify;
pub mod register;

#[derive(Debug, Serialize, Deserialize)]
pub struct Account(String);

#[derive(Debug, Serialize, Deserialize)]
pub struct ClientData {
    id: String,
    project_id: String,
    relay_url: String,
    sym_key: String,
}
