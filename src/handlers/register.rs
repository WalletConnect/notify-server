use {
    super::Account,
    crate::{handlers::ClientData, state::AppState},
    axum::{
        extract::{Json, State},
        http::StatusCode,
        response::{IntoResponse, Response},
    },
    hyper::HeaderMap,
    serde::{Deserialize, Serialize},
    std::sync::Arc,
};

#[derive(Serialize, Deserialize, Debug)]
// TODO: rename all camel case
pub struct RegisterBody {
    account: Account,
    #[serde(rename = "relayUrl")]
    #[serde(default = "default_relay_url")]
    relay_url: String,
    #[serde(rename = "symKey")]
    sym_key: String,
}

pub async fn handler(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(data): Json<RegisterBody>,
) -> Result<axum::response::Response, crate::error::Error> {
    let db = state.example_store.clone();

    let project_id = headers.get("Auth").unwrap().to_str().unwrap();

    let insert_data = ClientData {
        id: data.account.0.clone(), // format!("{}:{}", project_id, data.account.0)?,
        // TODO: This needs auth so that malicious user cannot add client's to other projects
        // project_id: project_id.to_string(), // project_id.to_string(),
        relay_url: data.relay_url,
        sym_key: data.sym_key,
    };

    db.collection::<ClientData>(project_id)
        .insert_one(insert_data, None)
        .await?;

    Ok((
        StatusCode::OK,
        format!("Successfully registered user {}", data.account.0),
    )
        .into_response())
}

fn default_relay_url() -> String {
    "wss://relay.walletconnect.com".to_string()
}
