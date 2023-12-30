use {
    crate::{
        error::{Error, Result},
        model::helpers::get_subscriber_accounts_and_scopes_by_project_id,
        rate_limit::{self, Clock},
        registry::{extractor::AuthedProjectId, storage::redis::Redis},
        state::AppState,
    },
    axum::{extract::State, http::StatusCode, response::IntoResponse, Json},
    relay_rpc::domain::ProjectId,
    std::sync::Arc,
    tracing::instrument,
};

#[instrument(name = "get_subscribers_v1", skip(state))]
pub async fn handler(
    State(state): State<Arc<AppState>>,
    AuthedProjectId(project_id, _): AuthedProjectId,
) -> Result<axum::response::Response> {
    if let Some(redis) = state.redis.as_ref() {
        get_subscribers_rate_limit(redis, &project_id, &state.clock).await?;
    }

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

pub async fn get_subscribers_rate_limit(
    redis: &Arc<Redis>,
    project_id: &ProjectId,
    clock: &Clock,
) -> Result<()> {
    rate_limit::token_bucket(
        redis,
        project_id.to_string(),
        5,
        chrono::Duration::seconds(1),
        1,
        clock,
    )
    .await
}
