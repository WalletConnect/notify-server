use {
    crate::{
        error::NotifyServerError, model::helpers::get_subscriber_accounts_by_project_id,
        registry::extractor::AuthedProjectId, state::AppState,
    },
    axum::{
        extract::State,
        http::StatusCode,
        response::{IntoResponse, Response},
        Json,
    },
    std::sync::Arc,
    tracing::instrument,
};

#[instrument(name = "get_subscribers_v0", skip(state))]
pub async fn handler(
    State(state): State<Arc<AppState>>,
    AuthedProjectId(project_id, _): AuthedProjectId,
) -> Result<Response, NotifyServerError> {
    let accounts =
        get_subscriber_accounts_by_project_id(project_id, &state.postgres, state.metrics.as_ref())
            .await
            .map_err(|e| match e {
                sqlx::Error::RowNotFound => {
                    NotifyServerError::BadRequest("Project not found".into())
                }
                e => e.into(),
            })?;

    Ok((StatusCode::OK, Json(accounts)).into_response())
}
