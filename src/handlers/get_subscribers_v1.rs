use {
    crate::{
        error::Result, extractors::AuthedProjectId,
        model::helpers::get_subscriber_accounts_and_scopes_by_project_id, state::AppState,
    },
    axum::{extract::State, http::StatusCode, response::IntoResponse, Json},
    std::sync::Arc,
    tracing::info,
};

pub async fn handler(
    State(state): State<Arc<AppState>>,
    AuthedProjectId(project_id, _): AuthedProjectId,
) -> Result<axum::response::Response> {
    info!("Getting subscribers for project: {project_id}");

    let accounts =
        get_subscriber_accounts_and_scopes_by_project_id(project_id, &state.postgres).await?;

    Ok((StatusCode::OK, Json(accounts)).into_response())
}
