use {
    crate::{
        analytics::follow_notification_link::FollowNotificationLinkParams,
        error::NotifyServerError,
        model::helpers::get_follow_notification_link,
        rate_limit::{self, Clock, RateLimitError},
        registry::storage::redis::Redis,
        state::AppState,
    },
    axum::{
        extract::{Path, State},
        response::{IntoResponse, Redirect, Response},
    },
    hyper::StatusCode,
    std::sync::Arc,
    tracing::instrument,
    url::Url,
    uuid::Uuid,
};

#[instrument(name = "follow_notification_link", skip_all)]
pub async fn handler(
    state: State<Arc<AppState>>,
    Path(subscriber_notification_id): Path<Uuid>,
) -> Result<Response, NotifyServerError> {
    if let Some(redis) = state.redis.as_ref() {
        rate_limit(redis, &subscriber_notification_id, &state.clock).await?;
    }

    let notification = get_follow_notification_link(
        subscriber_notification_id,
        &state.postgres,
        state.metrics.as_ref(),
    )
    .await?;

    if let Some(notification) = notification {
        if let Some(url) = notification.notification_url {
            state
                .analytics
                .follow_notification_links(FollowNotificationLinkParams {
                    project_pk: notification.project_pk,
                    project_id: notification.project_id,
                    subscriber_pk: notification.subscriber_pk,
                    subscriber_account: notification.subscriber_account,
                    notification_topic: notification.notification_topic,
                    subscriber_notification_id: notification.subscriber_notification_id,
                    notification_id: notification.notification_id,
                    notification_type: notification.notification_type,
                });

            Ok(Redirect::temporary(&url).into_response())
        } else {
            Ok((StatusCode::NOT_FOUND, "That link was not found.").into_response())
        }
    } else {
        Ok((StatusCode::NOT_FOUND, "That link was not found.").into_response())
    }
}

pub async fn rate_limit(
    redis: &Arc<Redis>,
    subscriber_notification_id: &Uuid,
    clock: &Clock,
) -> Result<(), RateLimitError> {
    rate_limit::token_bucket(
        redis,
        format!("follow-notification-link-{subscriber_notification_id}"),
        20,
        chrono::Duration::seconds(1),
        2,
        clock,
    )
    .await
}

pub fn format_follow_link(notify_url: &Url, subscriber_notification_id: &Uuid) -> Url {
    notify_url
        .join(&format!("/v1/notification/{subscriber_notification_id}"))
        .expect("Safe unwrap: inputs are valid URLs")
}
