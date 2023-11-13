use {
    crate::{
        error,
        error::Error,
        metrics::Metrics,
        model::{
            helpers::{get_project_by_project_id, get_subscribers_for_project_in},
            types::AccountId,
        },
        registry::extractor::AuthedProjectId,
        services::publisher_service::helpers::{
            upsert_notification, upsert_subscriber_notifications,
        },
        state::AppState,
        types::Notification,
    },
    axum::{extract::State, http::StatusCode, response::IntoResponse, Json},
    error::Result,
    serde::{Deserialize, Serialize},
    std::{collections::HashSet, sync::Arc, time::Instant},
    tracing::{info, instrument},
    uuid::Uuid,
    wc::metrics::otel::{Context, KeyValue},
};

pub type NotifyBody = Vec<NotifyBodyNotification>;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NotifyBodyNotification {
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

pub async fn handler(
    state: State<Arc<AppState>>,
    authed_project_id: AuthedProjectId,
    body: Json<NotifyBody>,
) -> Result<axum::response::Response> {
    let response = handler_impl(state, authed_project_id, body).await?;
    Ok((StatusCode::OK, Json(response)).into_response())
}

#[instrument(name = "notify_v1", skip(state, body))]
pub async fn handler_impl(
    State(state): State<Arc<AppState>>,
    AuthedProjectId(project_id, _): AuthedProjectId,
    Json(body): Json<NotifyBody>,
) -> Result<Response> {
    let start = Instant::now();

    let project = get_project_by_project_id(project_id.clone(), &state.postgres)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => Error::BadRequest("Project not found".into()),
            e => e.into(),
        })?;

    let mut response = Response {
        sent: HashSet::new(),
        failed: HashSet::new(),
        not_found: HashSet::new(),
    };

    // TODO looping here is not scalable for database inserts (e.g. thousands) for the same reason we need to insert with array for the subscriber notifications
    for body in body {
        let NotifyBodyNotification {
            notification_id,
            notification,
            accounts,
        } = body;

        notification.validate()?;

        let notification = upsert_notification(
            notification_id.unwrap_or_else(|| Uuid::new_v4().to_string()),
            project.id,
            notification,
            &state.postgres,
        )
        .await?;

        // We assume all accounts were not found until found
        response.not_found.extend(accounts.iter().cloned());

        // FIXME this is inefficient to get all subscribers when only a subset are in the request
        let subscribers =
            get_subscribers_for_project_in(project.id, &accounts, &state.postgres).await?;

        let mut subscriber_ids = Vec::with_capacity(subscribers.len());
        for subscriber in subscribers {
            let account = subscriber.account;
            response.not_found.remove(&account);

            if !subscriber.scope.contains(&notification.notification.r#type) {
                response.failed.insert(SendFailure {
                    account: account.clone(),
                    reason: "Client is not subscribed to this notification type".into(),
                });
                continue;
            }

            info!("Sending notification for {account}");
            subscriber_ids.push(subscriber.id);
            response.sent.insert(account);
        }

        upsert_subscriber_notifications(notification.id, &subscriber_ids, &state.postgres).await?;
    }

    info!("Response: {response:?} for /v1/notify from project: {project_id}");

    if let Some(metrics) = &state.metrics {
        send_metrics(metrics, &response, start);
    }

    Ok(response)
}

fn send_metrics(metrics: &Metrics, response: &Response, start: Instant) {
    let ctx = Context::current();
    metrics.dispatched_notifications.add(
        &ctx,
        response.sent.len() as u64,
        &[KeyValue::new("type", "sent")],
    );
    metrics.dispatched_notifications.add(
        &ctx,
        response.failed.len() as u64,
        &[KeyValue::new("type", "failed")],
    );
    metrics.dispatched_notifications.add(
        &ctx,
        response.not_found.len() as u64,
        &[KeyValue::new("type", "not_found")],
    );
    metrics
        .notify_latency
        .record(&ctx, start.elapsed().as_millis().try_into().unwrap(), &[])
}
