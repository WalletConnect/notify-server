use {
    crate::{
        error::{Error, Result},
        model::helpers::{
            get_project_by_project_id, set_welcome_notification, WelcomeNotification,
        },
        rate_limit::{self, Clock},
        registry::{extractor::AuthedProjectId, storage::redis::Redis},
        state::AppState,
    },
    axum::{extract::State, http::StatusCode, response::IntoResponse, Json},
    relay_rpc::domain::ProjectId,
    std::sync::Arc,
    tracing::instrument,
};

#[instrument(name = "post_welcome_notification", skip(state))]
pub async fn handler(
    State(state): State<Arc<AppState>>,
    AuthedProjectId(project_id, _): AuthedProjectId,
    Json(welcome_notification): Json<WelcomeNotification>,
) -> Result<axum::response::Response> {
    if let Some(redis) = state.redis.as_ref() {
        post_welcome_notification_rate_limit(redis, &project_id, &state.clock).await?;
    }

    // TODO combine queries for performance
    let project = get_project_by_project_id(project_id, &state.postgres, state.metrics.as_ref())
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => Error::BadRequest("Project not found".into()),
            e => e.into(),
        })?;

    set_welcome_notification(
        project.id,
        welcome_notification,
        &state.postgres,
        state.metrics.as_ref(),
    )
    .await?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn post_welcome_notification_rate_limit(
    redis: &Arc<Redis>,
    project_id: &ProjectId,
    clock: &Clock,
) -> Result<()> {
    rate_limit::token_bucket(
        redis,
        format!("post_welcome_notification-{project_id}"),
        100,
        chrono::Duration::minutes(1),
        1,
        clock,
    )
    .await
}
