use {
    crate::{error::Result, extractors::AuthedProjectId, state::AppState, types::ClientData},
    axum::{extract::State, http::StatusCode, response::IntoResponse, Json},
    futures::TryStreamExt,
    log::info,
    std::sync::Arc,
};

pub async fn handler(
    State(state): State<Arc<AppState>>,
    AuthedProjectId(project_id, _): AuthedProjectId,
) -> Result<axum::response::Response> {
    info!("Getting subscribers for project: {}", project_id);

    let mut cursor = state
        .database
        .collection::<ClientData>(&project_id)
        .find(None, None)
        .await?;

    let mut result = vec![];

    while let Some(client) = cursor.try_next().await? {
        result.push(client.id);
    }

    Ok((StatusCode::OK, Json(result)).into_response())
}
