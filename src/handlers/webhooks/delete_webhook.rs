use {
    crate::{error::Result, extractors::AuthedProjectId, state::AppState, types::WebhookInfo},
    axum::{
        extract::{Path, State},
        response::IntoResponse,
    },
    log::info,
    mongodb::bson::doc,
    std::sync::Arc,
    uuid::Uuid,
};

pub async fn handler(
    AuthedProjectId(project_id, _): AuthedProjectId,
    Path((_, webhook_id)): Path<(String, Uuid)>,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse> {
    let request_id = uuid::Uuid::new_v4();
    info!("[{request_id}] Deleting webhook: {webhook_id} for project: {project_id}");

    state
        .database
        .collection::<WebhookInfo>("webhooks")
        .delete_one(
            doc! {"project_id": project_id, "id": webhook_id.to_string()},
            None,
        )
        .await?;

    Ok(axum::http::StatusCode::OK.into_response())
}
