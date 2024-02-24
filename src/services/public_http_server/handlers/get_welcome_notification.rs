use {
    crate::{
        error::NotifyServerError,
        model::helpers::{get_project_by_project_id, get_welcome_notification},
        rate_limit::{self, Clock, RateLimitError},
        registry::{extractor::AuthedProjectId, storage::redis::Redis},
        state::AppState,
    },
    axum::{
        extract::State,
        response::{IntoResponse, Response},
        Json,
    },
    relay_rpc::domain::ProjectId,
    std::sync::Arc,
    tracing::instrument,
};

#[instrument(name = "get_welcome_notification", skip(state))]
pub async fn handler(
    State(state): State<Arc<AppState>>,
    AuthedProjectId(project_id, _): AuthedProjectId,
) -> Result<Response, NotifyServerError> {
    if let Some(redis) = state.redis.as_ref() {
        get_welcome_notification_rate_limit(redis, &project_id, &state.clock).await?;
    }

    let project = get_project_by_project_id(project_id, &state.postgres, state.metrics.as_ref())
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => {
                NotifyServerError::UnprocessableEntity("Project not found".into())
            }
            e => e.into(),
        })?;

    // TODO this should lookup by project_id not project UUID, but database can't differentiate between a missing project and a missing welcome notificvation?
    let welcome_notification =
        get_welcome_notification(project.id, &state.postgres, state.metrics.as_ref()).await?;

    Ok(Json(welcome_notification).into_response())
}

pub async fn get_welcome_notification_rate_limit(
    redis: &Arc<Redis>,
    project_id: &ProjectId,
    clock: &Clock,
) -> Result<(), RateLimitError> {
    rate_limit::token_bucket(
        redis,
        format!("get_welcome_notification-{project_id}"),
        100,
        chrono::Duration::minutes(1),
        1,
        clock,
    )
    .await
}
