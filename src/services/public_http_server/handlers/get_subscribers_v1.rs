use {
    crate::{
        error::{Error, Result},
        model::helpers::get_subscriber_accounts_and_scopes_by_project_id,
        registry::extractor::AuthedProjectId,
        state::AppState,
    },
    axum::{extract::State, http::StatusCode, response::IntoResponse, Json},
    std::sync::Arc,
    tracing::instrument,
};

// TODO rate limit each project to 1 per second with burst up to 5
#[instrument(name = "get_subscribers_v1", skip(state))]
pub async fn handler(
    State(state): State<Arc<AppState>>,
    AuthedProjectId(project_id, _): AuthedProjectId,
) -> Result<axum::response::Response> {
    let accounts = get_subscriber_accounts_and_scopes_by_project_id(
        project_id,
        &state.postgres,
        state.metrics.as_ref(),
    )
    .await
    .map_err(|e| match e {
        sqlx::Error::RowNotFound => Error::BadRequest("Project not found".into()),
        e => e.into(),
    })?;

    Ok((StatusCode::OK, Json(accounts)).into_response())
}
