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
}

pub async fn handler(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(data): Json<RegisterBody>,
) -> impl IntoResponse {
    let db = state.example_store.clone().database("cast");

    let project_id = headers.get("Auth").unwrap().to_str().unwrap();

    let insert_data = ClientData {
        id: data.account.0.clone(), // format!("{}:{}", project_id, data.account.0)?,
        // TODO: This needs auth so that malicious user cannot add client's to other projects
        project_id: project_id.to_string(), // project_id.to_string(),
        relay_url: data.relay_url,
        sym_key: data.sym_key,
    };

    db.collection::<ClientData>("clients")
        .insert_one(insert_data, None)
        .await
        .unwrap();

    (
        StatusCode::OK,
        format!("Successfully registered user {}", data.account.0),
    )
}
