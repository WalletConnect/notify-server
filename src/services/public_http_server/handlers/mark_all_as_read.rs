use {
    crate::{
        error::NotifyServerError,
        model::helpers::{get_project_by_project_id, mark_all_notifications_as_read_for_project},
        rate_limit::{self, Clock, RateLimitError},
        registry::{extractor::AuthedProjectId, storage::redis::Redis},
        state::AppState,
    },
    axum::{
        extract::State,
        http::StatusCode,
        response::{IntoResponse, Response},
    },
    relay_rpc::domain::ProjectId,
    std::sync::Arc,
    tracing::instrument,
};

#[instrument(name = "mark_all_as_read", skip(state))]
pub async fn handler(
    State(state): State<Arc<AppState>>,
    AuthedProjectId(project_id, _): AuthedProjectId,
) -> Result<Response, NotifyServerError> {
    if let Some(redis) = state.redis.as_ref() {
        mark_all_as_read_rate_limit(redis, &project_id, &state.clock).await?;
    }

    let project = get_project_by_project_id(project_id, &state.postgres, state.metrics.as_ref())
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => {
                NotifyServerError::UnprocessableEntity("Project not found".into())
            }
            e => e.into(),
        })?;

    mark_all_notifications_as_read_for_project(project.id, &state.postgres, state.metrics.as_ref())
        .await?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn mark_all_as_read_rate_limit(
    redis: &Arc<Redis>,
    project_id: &ProjectId,
    clock: &Clock,
) -> Result<(), RateLimitError> {
    rate_limit::token_bucket(
        redis,
        format!("mark_all_as_read-{project_id}"),
        100,
        chrono::Duration::minutes(1),
        1,
        clock,
    )
    .await
}
