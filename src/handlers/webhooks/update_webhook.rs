use {
    super::WebhookConfig,
    crate::{error::Result, extractors::AuthedProjectId, state::AppState, types::WebhookInfo},
    axum::{
        extract::{Path, State},
        response::IntoResponse,
        Json,
    },
    log::info,
    mongodb::{bson, bson::doc},
    std::sync::Arc,
    uuid::Uuid,
};

pub async fn handler(
    Path((_, webhook_id)): Path<(String, Uuid)>,
    AuthedProjectId(project_id, _): AuthedProjectId,
    State(state): State<Arc<AppState>>,
    Json(webhook_info): Json<WebhookConfig>,
) -> Result<impl IntoResponse> {
    let request_id = uuid::Uuid::new_v4();
    info!("[{request_id}] Updating webhook: {webhook_id} for project: {project_id}");
    state
        .database
        .collection::<WebhookInfo>("webhooks")
        .update_one(
            doc! {"project_id": project_id, "id": webhook_id.to_string()},
            doc! {"$set": {"url": webhook_info.url, "events": bson::to_bson(&webhook_info.events)? } },
            None,
        )
        .await?;

    Ok(axum::http::StatusCode::NO_CONTENT.into_response())
}
