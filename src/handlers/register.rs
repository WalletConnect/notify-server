use {
    super::Account,
    crate::{handlers::ClientData, state::AppState},
    axum::{
        extract::{Json, State},
        http::StatusCode,
        response::IntoResponse,
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
    relay_url: String,
    #[serde(rename = "symKey")]
    sym_key: String,
    #[serde(rename = "projectId")]
    project_id: String,
}

type Topic = Arc<str>;

/// Data structure representing PublishParams.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PublishParams {
    /// Topic to publish to.
    pub topic: Topic,
    /// Message to publish.
    pub message: Arc<str>,
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
    // #[serde(default, skip_serializing_if = "is_default")]
    pub prompt: bool,
}

pub struct JsonRpcPayload {
    id: String,
    jsonrpc: String,
    method: String,
    params: PublishParams,
}

pub async fn handler(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(data): Json<RegisterBody>,
) -> impl IntoResponse {
    let db = state.example_store.clone().database("cast");

    // let project_id = headers.get("Auth").unwrap().to_str().unwrap();

    let insert_data = ClientData {
        id: data.account.0, // format!("{}:{}", project_id, data.account.0)?,
        // TODO: This needs auth so that malicious user cannot add client's to other projects
        project_id: data.project_id, // project_id.to_string(),
        relay_url: data.relay_url,
        sym_key: data.sym_key,
    };

    db.collection::<ClientData>("clients")
        .insert_one(insert_data, None)
        .await
        .unwrap();

    (
        StatusCode::OK,
        format!(
            "OK, {} v{}",
            state.build_info.crate_info.name, state.build_info.crate_info.version
        ),
    )
}
