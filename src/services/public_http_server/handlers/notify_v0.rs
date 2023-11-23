use {
    super::notify_v1,
    crate::{
        error, model::types::AccountId, registry::extractor::AuthedProjectId, state::AppState,
        types::Notification,
    },
    axum::{extract::State, http::StatusCode, response::IntoResponse, Json},
    error::Result,
    serde::{Deserialize, Serialize},
    std::{collections::HashSet, sync::Arc},
    tracing::instrument,
};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NotifyBody {
    #[serde(default)]
    pub notification_id: Option<String>,
    pub notification: Notification,
    pub accounts: Vec<AccountId>,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
pub struct SendFailure {
    pub account: AccountId,
    pub reason: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Response {
    pub sent: HashSet<AccountId>,
    pub failed: HashSet<SendFailure>,
    pub not_found: HashSet<AccountId>,
}

#[instrument(name = "notify_v0", skip_all)]
pub async fn handler(
    state: State<Arc<AppState>>,
    authed_project_id: AuthedProjectId,
    Json(notify_args): Json<NotifyBody>,
) -> Result<axum::response::Response> {
    let response = notify_v1::handler_impl(
        state,
        authed_project_id,
        Json(vec![notify_v1::NotifyBodyNotification {
            notification_id: notify_args.notification_id,
            notification: notify_args.notification,
            accounts: notify_args.accounts,
        }]),
    )
    .await?;

    let response = Response {
        sent: response.sent,
        failed: response
            .failed
            .into_iter()
            .map(|failure| SendFailure {
                account: failure.account,
                reason: failure.reason,
            })
            .collect(),
        not_found: response.not_found,
    };

    Ok((StatusCode::OK, Json(response)).into_response())
}
