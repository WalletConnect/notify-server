use {
    crate::{handlers::ClientData, state::AppState},
    axum::{
        extract::{Json, Path, State},
        http::StatusCode,
        response::IntoResponse,
    },
    chacha20poly1305::{aead::generic_array::GenericArray, KeyInit},
    mongodb::bson::doc,
    opentelemetry::{Context, KeyValue},
    serde::{Deserialize, Serialize},
    std::sync::Arc,
};

#[derive(Serialize, Deserialize, Debug)]
// TODO: rename all camel case
#[serde(rename_all = "camelCase")]
pub struct RegisterBody {
    pub account: String,
    #[serde(default = "default_relay_url")]
    pub relay_url: String,
    pub sym_key: String,
}

pub async fn handler(
    Path(project_id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(data): Json<RegisterBody>,
) -> Result<axum::response::Response, crate::error::Error> {
    let db = state.database.clone();
    let url = url::Url::parse(&data.relay_url)?;

    if url.scheme() != "wss" {
        return Ok((
            StatusCode::BAD_REQUEST,
            "Invalid procotol. Only \"wss://\" is accepted.",
        )
            .into_response());
    }

    // Test the key
    let key = hex::decode(data.sym_key.clone())?;
    chacha20poly1305::ChaCha20Poly1305::new(GenericArray::from_slice(&key));

    // Construct documentDB entry
    let insert_data = ClientData {
        id: data.account.clone(),
        relay_url: url.to_string().trim_end_matches('/').to_string(),
        sym_key: data.sym_key,
    };

    // Insert data
    // Temporary replaced to allow easier developement
    if db
        .collection::<ClientData>(&project_id)
        .insert_one(&insert_data, None)
        .await
        .is_err()
    {
        // This will create new entry or update existing one
        // Should be replaced with `insert_one` in the future to avoid overwriting
        db.collection::<ClientData>(&project_id)
            .replace_one(doc! { "_id": data.account.clone()}, insert_data, None)
            .await?;
    };

    if let Some(metrics) = &state.metrics {
        metrics
            .registered_clients
            .add(&Context::current(), 1, &[KeyValue::new(
                "project_id",
                project_id,
            )])
    }

    Ok((
        StatusCode::CREATED,
        format!("Successfully registered user {}", data.account),
    )
        .into_response())
}

// TODO: Load this from env
fn default_relay_url() -> String {
    "wss://relay.walletconnect.com".to_string()
}
