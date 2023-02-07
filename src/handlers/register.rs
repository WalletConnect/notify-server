use {
    super::Account,
    crate::{handlers::ClientData, state::AppState},
    axum::{
        extract::{Json, Path, State},
        http::StatusCode,
        response::IntoResponse,
    },
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
    // headers: HeaderMap,
    Path(project_id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(data): Json<RegisterBody>,
) -> Result<axum::response::Response, crate::error::Error> {
    let db = state.example_store.clone();

    // Verify that the url is proper url and starting with websocket
    // match url::Url::parse(&data.relay_url) {
    //     Err(_) => return todo!(),
    //     Ok(url) => {
    //         if url.scheme() != "wss" {
    //             return todo!();
    //         }
    //     }
    // };

    if url::Url::parse(&data.relay_url)?.scheme() != "wss" {
        return Ok((
            StatusCode::BAD_REQUEST,
            "Invalid procotol. Only \"wss://\" is accepted.",
        )
            .into_response());
    }

    // Construct documentDB entry
    let insert_data = ClientData {
        id: data.account.0.clone(),
        relay_url: data.relay_url,
        sym_key: data.sym_key,
    };

    // Insert data
    db.collection::<ClientData>(&project_id)
        .insert_one(insert_data, None)
        .await?;

    Ok((
        StatusCode::CREATED,
        format!("Successfully registered user {}", data.account.0),
    )
        .into_response())
}

fn default_relay_url() -> String {
    "wss://relay.walletconnect.com".to_string()
}
