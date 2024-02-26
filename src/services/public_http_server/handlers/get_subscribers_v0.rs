use {
    crate::{
        error::NotifyServerError,
        model::helpers::get_subscriber_accounts_by_project_id,
        rate_limit::{self, Clock, RateLimitError},
        registry::{extractor::AuthedProjectId, storage::redis::Redis},
        state::AppState,
    },
    axum::{
        extract::State,
        http::StatusCode,
        response::{IntoResponse, Response},
        Json,
    },
    relay_rpc::domain::ProjectId,
    std::sync::Arc,
    tracing::instrument,
};

#[instrument(name = "get_subscribers_v0", skip(state))]
pub async fn handler(
    State(state): State<Arc<AppState>>,
    AuthedProjectId(project_id, _): AuthedProjectId,
) -> Result<Response, NotifyServerError> {
    if let Some(redis) = state.redis.as_ref() {
        get_subscribers_rate_limit(redis, &project_id, &state.clock).await?;
    }

    let accounts =
        get_subscriber_accounts_by_project_id(project_id, &state.postgres, state.metrics.as_ref())
            .await
            .map_err(|e| match e {
                sqlx::Error::RowNotFound => {
                    NotifyServerError::UnprocessableEntity("Project not found".into())
                }
                e => e.into(),
            })?;

    Ok((StatusCode::OK, Json(accounts)).into_response())
}
pub async fn get_subscribers_rate_limit(
    redis: &Arc<Redis>,
    project_id: &ProjectId,
    clock: &Clock,
) -> Result<(), RateLimitError> {
    rate_limit::token_bucket(
        redis,
        format!("subscribers-v0-{project_id}"),
        5,
        chrono::Duration::seconds(1),
        1,
        clock,
    )
    .await
}
