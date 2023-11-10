use {
    crate::{
        error::Result, model::helpers::get_subscriber_accounts_by_project_id,
        registry::extractor::AuthedProjectId, state::AppState,
    },
    axum::{extract::State, http::StatusCode, response::IntoResponse, Json},
    std::sync::Arc,
};

pub async fn handler(
    State(state): State<Arc<AppState>>,
    AuthedProjectId(project_id, _): AuthedProjectId,
) -> Result<axum::response::Response> {
    let accounts = get_subscriber_accounts_by_project_id(project_id, &state.postgres).await?;

    Ok((StatusCode::OK, Json(accounts)).into_response())
}
