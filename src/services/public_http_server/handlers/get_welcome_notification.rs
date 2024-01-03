use {
    crate::{
        error::{Error, Result},
        model::helpers::{get_project_by_project_id, get_welcome_notification},
        rate_limit::{self, Clock},
        registry::{extractor::AuthedProjectId, storage::redis::Redis},
        state::AppState,
    },
    axum::{extract::State, http::StatusCode, response::IntoResponse, Json},
    relay_rpc::domain::ProjectId,
    std::sync::Arc,
    tracing::instrument,
};

#[instrument(name = "get_welcome_notification", skip(state))]
pub async fn handler(
    State(state): State<Arc<AppState>>,
    AuthedProjectId(project_id, _): AuthedProjectId,
) -> Result<axum::response::Response> {
    if let Some(redis) = state.redis.as_ref() {
        get_welcome_notification_rate_limit(redis, &project_id, &state.clock).await?;
    }

    let project = get_project_by_project_id(project_id, &state.postgres, state.metrics.as_ref())
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => Error::BadRequest("Project not found".into()),
            e => e.into(),
        })?;

    // TODO this should lookup by project_id not project UUID, but database can't differentiate between a missing project and a missing welcome notificvation?
    // Should a missing welcome notification return a 404, an empty welcome notification, or a different JSON response?
    // 404 results in errors in JS console...
    let welcome_notification =
        get_welcome_notification(project.id, &state.postgres, state.metrics.as_ref()).await?;

    if let Some(welcome_notification) = welcome_notification {
        Ok((StatusCode::OK, Json(welcome_notification)).into_response())
    } else {
        Ok(StatusCode::NOT_FOUND.into_response())
    }
}

pub async fn get_welcome_notification_rate_limit(
    redis: &Arc<Redis>,
    project_id: &ProjectId,
    clock: &Clock,
) -> Result<()> {
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
