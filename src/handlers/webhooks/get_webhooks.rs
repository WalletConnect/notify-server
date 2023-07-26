use {
    super::WebhookConfig,
    crate::{error::Result, extractors::AuthedProjectId, state::AppState, types::WebhookInfo},
    axum::{extract::State, response::IntoResponse, Json},
    futures::TryStreamExt,
    log::info,
    mongodb::bson::doc,
    std::{collections::HashMap, sync::Arc},
};

pub async fn handler(
    AuthedProjectId(project_id, _): AuthedProjectId,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse> {
    let request_id = uuid::Uuid::new_v4();
    info!("[{request_id}] Getting webhooks for project: {project_id}");

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

    Ok((axum::http::StatusCode::OK, Json(webhooks)).into_response())
}
