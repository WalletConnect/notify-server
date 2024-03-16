use {
    crate::{
        analytics::notification_link::NotificationLinkParams,
        error::NotifyServerError,
        model::helpers::get_notification_link,
        rate_limit::{self, Clock, RateLimitError},
        registry::storage::redis::Redis,
        state::AppState,
    },
    axum::{
        extract::{Path, State},
        response::{IntoResponse, Redirect, Response},
    },
    hyper::{header::ToStrError, HeaderMap, StatusCode},
    std::{
        net::{AddrParseError, IpAddr},
        sync::Arc,
    },
    thiserror::Error,
    tracing::{debug, error, instrument},
    url::Url,
    uuid::Uuid,
    wc::geoip::{self, MaxMindResolver, MaxMindResolverError, Resolver},
};

#[instrument(name = "follow_notification_link", skip_all)]
pub async fn handler(
    state: State<Arc<AppState>>,
    Path(subscriber_notification_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Response, NotifyServerError> {
    if let Some(redis) = state.redis.as_ref() {
        rate_limit(redis, &subscriber_notification_id, &state.clock).await?;
    }

    let notification = get_notification_link(
        subscriber_notification_id,
        &state.postgres,
        state.metrics.as_ref(),
    )
    .await?;

    let geo = match state
        .geoip_resolver
        .as_ref()
        .map(|resolver| get_geo_from_x_forwarded_for(&headers, resolver.as_ref()))
        .transpose()
    {
        Ok(geo) => geo,
        Err(e) => {
            error!("Failed to get geo from X-Forwarded-For header: {e}");
            None
        }
    };

    let user_agent = match headers.get("User-Agent") {
        Some(value) => match value.to_str() {
            Ok(value) => Some(value.to_owned()),
            Err(e) => {
                debug!("Failed to get User-Agent header as string: {e}");
                None
            }
        },
        None => None,
    };

    if let Some(notification) = notification {
        if let Some(url) = notification.notification_url {
            state.analytics.notification_links(NotificationLinkParams {
                project_pk: notification.project_pk,
                project_id: notification.project_id,
                subscriber_pk: notification.subscriber_pk,
                subscriber_account: notification.subscriber_account,
                notification_topic: notification.notification_topic,
                subscriber_notification_pk: notification.subscriber_notification_id,
                notification_pk: notification.notification_id,
                notification_type: notification.notification_type,
                geo,
                user_agent,
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

#[derive(Debug, Error)]
pub enum GetGeoError {
    #[error("Missing x-forwarded-for header")]
    MissingXForwardedFor,

    #[error("x-forwarded-for not string: {0}")]
    XForwardedForNotString(ToStrError),

    #[error("No first item in x-forwarded-for header")]
    NoItemsInXForwardedFor,

    #[error("First item not IP addres: {0}")]
    FirstItemNotIpAddr(AddrParseError),

    #[error("MaxMind resolver: {0}")]
    MaxMindResolver(MaxMindResolverError),
}

pub fn get_geo_from_x_forwarded_for(
    headers: &HeaderMap,
    geoip_resolver: &MaxMindResolver,
) -> Result<geoip::Data, GetGeoError> {
    let header = headers
        .get("X-Forwarded-For")
        .ok_or(GetGeoError::MissingXForwardedFor)?;
    let value = header
        .to_str()
        .map_err(GetGeoError::XForwardedForNotString)?;
    let ip = value
        .split(',')
        .next()
        .ok_or(GetGeoError::NoItemsInXForwardedFor)?;
    let ip = ip
        .trim()
        .parse::<IpAddr>()
        .map_err(GetGeoError::FirstItemNotIpAddr)?;
    geoip_resolver
        .lookup_geo_data(ip)
        .map_err(GetGeoError::MaxMindResolver)
}
