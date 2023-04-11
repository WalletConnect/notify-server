use {
    crate::{
        state::AppState,
        types::{ClientData, LookupEntry, RegisterBody},
        unregister_service::UnregisterMessage,
    },
    axum::{
        extract::{Json, Path, State},
        http::StatusCode,
        response::IntoResponse,
    },
    chacha20poly1305::{aead::generic_array::GenericArray, KeyInit},
    mongodb::{bson::doc, options::ReplaceOptions},
    opentelemetry::{Context, KeyValue},
    std::sync::Arc,
};
pub async fn handler(
    Path(project_id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(data): Json<RegisterBody>,
) -> Result<axum::response::Response, crate::error::Error> {
    let db = state.database.clone();
    let url = url::Url::parse(&data.relay_url)?;

    #[cfg(test)]
    if url.scheme() != "wss" {
        return Ok((
            StatusCode::BAD_REQUEST,
            "Invalid procotol. Only \"wss://\" is accepted.",
        )
            .into_response());
    }

    // Test the key
    let key = hex::decode(data.sym_key.clone())?;
    let topic = sha256::digest(&*key);
    chacha20poly1305::ChaCha20Poly1305::new(GenericArray::from_slice(&key));

    // Construct documentDB entry
    let insert_data = ClientData {
        id: data.account.clone(),
        relay_url: url.to_string().trim_end_matches('/').to_string(),
        sym_key: data.sym_key,
    };

    // Currently overwriting the document if it exists,
    // but we should probably just update the fields
    db.collection::<ClientData>(&project_id)
        .replace_one(
            doc! { "_id": data.account.clone()},
            insert_data,
            ReplaceOptions::builder().upsert(true).build(),
        )
        .await?;

    // TODO: Replace with const
    db.collection::<LookupEntry>("lookup_table")
        .replace_one(
            doc! { "_id": &topic},
            LookupEntry {
                topic: topic.clone(),
                project_id: project_id.clone(),
                account: data.account.clone(),
            },
            ReplaceOptions::builder().upsert(true).build(),
        )
        .await?;

    state
        .unregister_tx
        .send(UnregisterMessage::Register(topic))
        .await
        .unwrap();

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
