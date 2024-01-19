use {
    crate::{
        error::NotifyServerError,
        metrics::Metrics,
        model::{
            helpers::{
                get_project_by_project_id, get_subscribers_for_project_in, NotifySubscriberInfo,
            },
            types::AccountId,
        },
        rate_limit::{self, Clock, InternalRateLimitError, RateLimitError},
        registry::{extractor::AuthedProjectId, storage::redis::Redis},
        services::publisher_service::helpers::{
            upsert_notification, upsert_subscriber_notifications,
        },
        state::AppState,
        types::Notification,
        utils::get_address_from_account,
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
        time::Instant,
    },
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
pub struct ResponseBody {
    pub sent: HashSet<AccountId>,
    pub failed: HashSet<SendFailure>,
    pub not_found: HashSet<AccountId>,
}

pub async fn handler(
    state: State<Arc<AppState>>,
    authed_project_id: AuthedProjectId,
    body: Json<NotifyBody>,
) -> Result<Response, NotifyServerError> {
    let response = handler_impl(state, authed_project_id, body).await?;
    Ok((StatusCode::OK, Json(response)).into_response())
}

#[instrument(name = "notify_v1", skip(state, body))]
pub async fn handler_impl(
    State(state): State<Arc<AppState>>,
    AuthedProjectId(project_id, _): AuthedProjectId,
    Json(body): Json<NotifyBody>,
) -> Result<ResponseBody, NotifyServerError> {
    let start = Instant::now();

    if let Some(redis) = state.redis.as_ref() {
        notify_rate_limit(redis, &project_id, &state.clock).await?;
    }

    for notification in &body {
        notification.notification.validate()?;
    }

    info!("notification count: {}", body.len());
    let subscriber_notifications = body
        .iter()
        .flat_map(|n| n.accounts.clone())
        .collect::<Vec<_>>();
    let subscriber_notification_count = subscriber_notifications.len();
    info!("subscriber_notification_count: {subscriber_notification_count}");
    const SUBSCRIBER_NOTIFICATION_COUNT_LIMIT: usize = 500;
    if subscriber_notification_count > SUBSCRIBER_NOTIFICATION_COUNT_LIMIT {
        return Err(NotifyServerError::BadRequest(
            format!("Too many notifications: {subscriber_notification_count} > {SUBSCRIBER_NOTIFICATION_COUNT_LIMIT}")
        ));
    }

    let project =
        get_project_by_project_id(project_id.clone(), &state.postgres, state.metrics.as_ref())
            .await
            .map_err(|e| match e {
                sqlx::Error::RowNotFound => {
                    NotifyServerError::BadRequest("Project not found".into())
                }
                e => e.into(),
            })?;

    // TODO this response is not per-notification
    let mut response = ResponseBody {
        sent: HashSet::new(),
        failed: HashSet::new(),
        not_found: HashSet::new(),
    };

    for NotifyBodyNotification {
        notification_id,
        notification,
        accounts,
    } in body
    {
        let notification = upsert_notification(
            notification_id.unwrap_or_else(|| Uuid::new_v4().to_string()),
            project.id,
            notification,
            &state.postgres,
            state.metrics.as_ref(),
        )
        .await?;

        // We assume all accounts were not found until found
        response.not_found.extend(accounts.iter().cloned());

        let subscribers = {
            let chain_agnostic_lookup_table = accounts
                .iter()
                .map(|account| {
                    (
                        get_address_from_account(account).to_owned(),
                        account.clone(),
                    )
                })
                .collect::<HashMap<_, _>>();
            get_subscribers_for_project_in(
                project.id,
                &accounts,
                &state.postgres,
                state.metrics.as_ref(),
            )
            .await?
            .into_iter()
            .map(|subscriber| NotifySubscriberInfo {
                account: chain_agnostic_lookup_table
                    .get(get_address_from_account(&subscriber.account))
                    .unwrap()
                    .clone(),
                ..subscriber
            })
            .collect::<Vec<_>>()
        };

        let mut valid_subscribers = Vec::with_capacity(subscribers.len());
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

            valid_subscribers.push((subscriber.id, account));
        }

        let valid_subscribers = if let Some(redis) = state.redis.as_ref() {
            let result = subscriber_rate_limit(
                redis,
                &project_id,
                valid_subscribers
                    .iter()
                    .map(|(subscriber_id, _account)| *subscriber_id),
                &state.clock,
            )
            .await
            .map_err(NotifyServerError::RateLimitError)?;

            let mut valid_subscribers2 = Vec::with_capacity(valid_subscribers.len());
            for (subscriber_id, account) in valid_subscribers.into_iter() {
                let key = subscriber_rate_limit_key(&project_id, &subscriber_id);
                let (remaining, _reset) = result
                    .get(&key)
                    .expect("rate limit key expected in response");
                if remaining.is_negative() {
                    response.failed.insert(SendFailure {
                        account: account.clone(),
                        reason: "Rate limit exceeded".into(),
                    });
                } else {
                    valid_subscribers2.push((subscriber_id, account));
                }
            }
            valid_subscribers2
        } else {
            valid_subscribers
        };

        let mut subscriber_ids = Vec::with_capacity(valid_subscribers.len());
        for (subscriber_id, account) in valid_subscribers {
            subscriber_ids.push(subscriber_id);
            response.sent.insert(account);
        }

        upsert_subscriber_notifications(
            notification.id,
            &subscriber_ids,
            &state.postgres,
            state.metrics.as_ref(),
        )
        .await?;
    }

    info!("Response: {response:?} for /v1/notify from project: {project_id}");

    if let Some(metrics) = &state.metrics {
        send_metrics(metrics, &response, start);
    }

    Ok(response)
}

fn send_metrics(metrics: &Metrics, response: &ResponseBody, start: Instant) {
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

pub async fn notify_rate_limit(
    redis: &Arc<Redis>,
    project_id: &ProjectId,
    clock: &Clock,
) -> Result<(), RateLimitError> {
    rate_limit::token_bucket(
        redis,
        format!("notify-v1-{project_id}"),
        20,
        chrono::Duration::seconds(1),
        2,
        clock,
    )
    .await
}

type SubscriberRateLimitKey = String;

pub fn subscriber_rate_limit_key(
    project_id: &ProjectId,
    subscriber: &Uuid,
) -> SubscriberRateLimitKey {
    format!("notify-v1-subscriber-{project_id}:{subscriber}")
}

pub async fn subscriber_rate_limit(
    redis: &Arc<Redis>,
    project_id: &ProjectId,
    subscribers: impl IntoIterator<Item = Uuid>,
    clock: &Clock,
) -> Result<HashMap<SubscriberRateLimitKey, (i64, u64)>, InternalRateLimitError> {
    let keys = subscribers
        .into_iter()
        .map(|subscriber| subscriber_rate_limit_key(project_id, &subscriber))
        .collect::<Vec<_>>();
    rate_limit::token_bucket_many(redis, keys, 50, chrono::Duration::hours(1), 2, clock).await
}
