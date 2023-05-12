use {
    super::WebhookConfig,
    crate::{error::Result, state::AppState, types::WebhookInfo},
    axum::{
        extract::{Path, State},
        response::IntoResponse,
        Json,
    },
    futures::TryStreamExt,
    log::info,
    mongodb::bson::doc,
    std::{collections::HashMap, sync::Arc},
};

pub async fn handler(
    Path(project_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse> {
    info!("Getting webhooks for project: {}", project_id);

    let cursor = state
        .database
        .collection::<WebhookInfo>("webhooks")
        .find(doc! {"project_id": project_id}, None)
        .await?;

    let webhooks: HashMap<_, _> = cursor
        .into_stream()
        .map_ok(|webhook| {
            (webhook.id, WebhookConfig {
                url: webhook.url,
                events: webhook.events,
            })
        })
        .try_collect()
        .await?;

    Ok((axum::http::StatusCode::OK, Json(webhooks)))
}
