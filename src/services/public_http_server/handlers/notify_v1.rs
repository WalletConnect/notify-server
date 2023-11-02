use {
    crate::{
        error,
        metrics::Metrics,
        model::{
            helpers::{get_project_by_project_id, get_subscribers_for_project_in},
            types::AccountId,
        },
        registry::extractor::AuthedProjectId,
        services::publisher_service::helpers::{
            upsert_notification, upsert_subcriber_notification,
        },
        state::AppState,
        types::Notification,
    },
    axum::{extract::State, http::StatusCode, response::IntoResponse, Json},
    error::Result,
    serde::{Deserialize, Serialize},
    std::{collections::HashSet, sync::Arc, time::Instant},
    tracing::info,
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

// TODO remove
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
    State(state): State<Arc<AppState>>,
    AuthedProjectId(project_id, _): AuthedProjectId,
    Json(body): Json<NotifyBody>,
) -> Result<axum::response::Response> {
    let start = Instant::now();

    // TODO handle project not found
    let project = get_project_by_project_id(project_id.clone(), &state.postgres).await?;

    let mut response = Response {
        sent: HashSet::new(),
        failed: HashSet::new(),
        not_found: HashSet::new(),
    };

    for body in body {
        let NotifyBodyNotification {
            notification_id,
            notification,
            accounts,
        } = body;
        let notification = upsert_notification(
            notification_id.unwrap_or_else(|| Uuid::new_v4().to_string()),
            project.id,
            notification,
            &state.postgres,
        )
        .await?;

        // We assume all accounts were not found until found
        response.not_found.extend(accounts.iter().cloned());

        let subscribers =
            get_subscribers_for_project_in(project.id, &accounts, &state.postgres).await?;

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

            info!("Sending notification for {account}",);

            upsert_subcriber_notification(notification.id, subscriber.id, &state.postgres).await?;

            info!("Successfully sent notification to {account}");
            response.sent.insert(account);

            // TODO do in publisher service
            // state.analytics.message(SubscriberNotificationParams {
            //     project_pk: project.id,
            //     project_id,
            //     subscriber_pk: subscriber.id,
            //     account,
            //     notification_type: notification.notification.r#type,
            //     notify_topic: subscriber.topic,
            //     message_id: msg_id.into(),
            // });

            // warn!(
            //     "[{request_id}] Error sending notification to account {account} on topic: \
            //      {topic}: {error:?}"
            // );
            // response.failed.insert(SendFailure {
            //     account,
            //     reason: "Internal error".into(),
            // });
        }
    }

    info!("Response: {response:?} for /v1/notify from project: {project_id}");

    if let Some(metrics) = &state.metrics {
        send_metrics(metrics, &response, start);
    }

    Ok((StatusCode::OK, Json(())).into_response())
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
