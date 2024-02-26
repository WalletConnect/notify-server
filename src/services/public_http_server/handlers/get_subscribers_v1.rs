use {
    crate::{
        error::NotifyServerError,
        model::{helpers::get_subscribers_by_project_id_and_accounts, types::AccountId},
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
    serde::{Deserialize, Serialize},
    std::{
        collections::{HashMap, HashSet},
        sync::Arc,
    },
    tracing::instrument,
    uuid::Uuid,
    validator::Validate,
};

#[derive(Debug, Serialize, Deserialize, Clone, Validate)]
pub struct GetSubscribersBody {
    #[validate(length(min = 1, max = 100))]
    pub accounts: Vec<AccountId>,
}

impl GetSubscribersBody {
    pub fn validate(&self) -> Result<(), NotifyServerError> {
        Validate::validate(&self)
            .map_err(|error| NotifyServerError::UnprocessableEntity(error.to_string()))
    }
}

pub type GetSubscribersResponse = HashMap<AccountId, GetSubscribersResponseEntry>;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct GetSubscribersResponseEntry {
    pub notification_types: HashSet<Uuid>,
}

#[instrument(name = "get_subscribers_v1", skip(state))]
pub async fn handler(
    State(state): State<Arc<AppState>>,
    AuthedProjectId(project_id, _): AuthedProjectId,
    Json(body): Json<GetSubscribersBody>,
) -> Result<Response, NotifyServerError> {
    if let Some(redis) = state.redis.as_ref() {
        get_subscribers_rate_limit(redis, &project_id, &state.clock).await?;
    }

    body.validate()?;

    let accounts = get_subscribers_by_project_id_and_accounts(
        project_id,
        &body.accounts,
        &state.postgres,
        state.metrics.as_ref(),
    )
    .await
    .map_err(|e| match e {
        sqlx::Error::RowNotFound => {
            NotifyServerError::UnprocessableEntity("Project not found".into())
        }
        e => e.into(),
    })?;

    let response: GetSubscribersResponse = accounts
        .into_iter()
        .map(|subscriber| {
            (
                subscriber.account,
                GetSubscribersResponseEntry {
                    notification_types: subscriber.scope,
                },
            )
        })
        .collect();

    Ok((StatusCode::OK, Json(response)).into_response())
}

pub async fn get_subscribers_rate_limit(
    redis: &Arc<Redis>,
    project_id: &ProjectId,
    clock: &Clock,
) -> Result<(), RateLimitError> {
    rate_limit::token_bucket(
        redis,
        format!("subscribers-v1-{project_id}"),
        100,
        chrono::Duration::seconds(1),
        100,
        clock,
    )
    .await
}
