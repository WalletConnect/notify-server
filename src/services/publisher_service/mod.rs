use {
    self::helpers::{pick_subscriber_notification_for_processing, NotificationToProcess},
    super::public_http_server::handlers::notification_link::format_follow_link,
    crate::{
        analytics::{subscriber_notification::SubscriberNotificationParams, NotifyAnalytics},
        auth::{DidWeb, SignJwtError},
        error::NotifyServerError,
        metrics::Metrics,
        notify_message::{sign_message, JwtNotification, ProjectSigningDetails},
        publish_relay_message::publish_relay_message,
        rpc::{decode_key, DecodeKeyError, JsonRpcRequest, NotifyMessageAuth},
        spec::{NOTIFY_MESSAGE_METHOD, NOTIFY_MESSAGE_TAG, NOTIFY_MESSAGE_TTL},
        types::{Envelope, EnvelopeType0},
        utils::topic_from_key,
    },
    base64::Engine,
    chrono::{DateTime, Utc},
    helpers::{dead_letter_give_up_check, update_message_processing_status},
    relay_client::http::Client,
    relay_rpc::{
        auth::ed25519_dalek::SigningKey,
        domain::DecodedClientId,
        rpc::{msg_id::MsgId, Publish},
    },
    sqlx::{postgres::PgListener, PgPool},
    std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        time::Duration,
    },
    thiserror::Error,
    tokio::time::{interval, timeout},
    tracing::{error, info, instrument, warn},
    types::SubscriberNotificationStatus,
    url::Url,
};

pub mod helpers;
pub mod types;

// TODO: These should be configurable, add to the config
/// Maximum of the parallel messages processing workers
const MAX_WORKERS: usize = 50;
/// Number of workers to be spawned on the service start to clean the queue
const START_WORKERS: usize = 50;
// Messages queue stats observing database polling interval
const QUEUE_STATS_POLLING_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);
/// Maximum publishing time before the publish will be considered as failed
/// and the messages in queue with the `processing` state will be returned to the queue
const PUBLISHING_TIMEOUT: Duration = Duration::from_secs(60 * 5); // 5 minutes
/// Interval to check for dead letters
const DEAD_LETTER_POLL_INTERVAL: Duration = Duration::from_secs(60);
/// Total maximum time to process the message before it will be considered as failed
const PUBLISHING_GIVE_UP_TIMEOUT: Duration = Duration::from_secs(60 * 60 * 24); // One day

pub const NOTIFICATION_FOR_DELIVERY: &str = "notification_for_delivery";

#[instrument(skip_all)]
pub async fn start(
    notify_url: Url,
    postgres: PgPool,
    relay_client: Arc<Client>,
    metrics: Option<Metrics>,
    analytics: NotifyAnalytics,
) -> Result<(), sqlx::Error> {
    // Spawning a new task to observe messages queue stats by polling and export them to metrics
    if let Some(metrics) = metrics.clone() {
        tokio::spawn({
            let postgres = postgres.clone();
            async move {
                let mut interval = tokio::time::interval(QUEUE_STATS_POLLING_INTERVAL);
                loop {
                    interval.tick().await;
                    helpers::update_metrics_on_queue_stats(&metrics, &postgres).await;
                }
            }
        });
    }

    // Spawning dead letters check task
    tokio::spawn({
        let postgres = postgres.clone();
        let metrics = metrics.clone();
        async move {
            let mut poll_interval = interval(DEAD_LETTER_POLL_INTERVAL);
            loop {
                poll_interval.tick().await;
                if let Err(e) =
                    helpers::dead_letters_check(PUBLISHING_TIMEOUT, &postgres, metrics.as_ref())
                        .await
                {
                    warn!("Error on dead letters check: {:?}", e);
                }
            }
        }
    });

    let spawned_tasks_counter = Arc::new(AtomicUsize::new(START_WORKERS));

    // Spawning initial workers to process messages from the queue in case
    // if the service was down before and we need to clear the queue
    for _ in 0..START_WORKERS {
        // Spawning a new task to process the messages from the queue
        tokio::spawn({
            let notify_url = notify_url.clone();
            let postgres = postgres.clone();
            let relay_client = relay_client.clone();
            let spawned_tasks_counter = spawned_tasks_counter.clone();
            let metrics = metrics.clone();
            let analytics = analytics.clone();
            async move {
                process_and_handle(
                    &notify_url,
                    &postgres,
                    relay_client,
                    metrics.as_ref(),
                    &analytics,
                    spawned_tasks_counter,
                )
                .await;
            }
        });
    }

    let mut pg_notify_listener = PgListener::connect_with(&postgres).await?;
    pg_notify_listener.listen(NOTIFICATION_FOR_DELIVERY).await?;

    loop {
        // Blocking waiting for the notification of the new message in a queue
        pg_notify_listener.recv().await?;

        if spawned_tasks_counter.load(Ordering::Acquire) < MAX_WORKERS {
            let prev_spawned_tasks = spawned_tasks_counter.fetch_add(1, Ordering::Release);
            let new_spawned_tasks = prev_spawned_tasks + 1;
            if let Some(metrics) = metrics.as_ref() {
                metrics
                    .publishing_workers_count
                    .observe(new_spawned_tasks as u64, &[]);
                // TODO: Add worker execution time metric
            }

            // Spawning a new task to process the messages from the queue
            tokio::spawn({
                let notify_url = notify_url.clone();
                let postgres = postgres.clone();
                let relay_client = relay_client.clone();
                let spawned_tasks_counter = spawned_tasks_counter.clone();
                let metrics = metrics.clone();
                let analytics = analytics.clone();
                async move {
                    process_and_handle(
                        &notify_url,
                        &postgres,
                        relay_client,
                        metrics.as_ref(),
                        &analytics,
                        spawned_tasks_counter,
                    )
                    .await;
                }
            });
        } else {
            info!(
                "Max number of workers {} reached, not spawning more",
                MAX_WORKERS
            );
        }
    }
}

/// This function runs the process and properly handles
/// the spawned tasks counter and metrics
async fn process_and_handle(
    notify_url: &Url,
    postgres: &PgPool,
    relay_client: Arc<Client>,
    metrics: Option<&Metrics>,
    analytics: &NotifyAnalytics,
    spawned_tasks_counter: Arc<AtomicUsize>,
) {
    if let Err(e) =
        process_queued_messages(notify_url, postgres, relay_client, metrics, analytics).await
    {
        if let Some(metrics) = metrics {
            metrics.publishing_workers_errors.add(1, &[]);
        }
        warn!("Error on processing queued messages by the worker: {:?}", e);
    }

    let prev_spawned_tasks = spawned_tasks_counter.fetch_sub(1, Ordering::Relaxed);
    let new_spawned_tasks = prev_spawned_tasks - 1;

    if let Some(metrics) = metrics {
        metrics
            .publishing_workers_count
            .observe(new_spawned_tasks as u64, &[]);
    }
}

/// Picking messages from the queue and processing them in a loop until
/// there are no more messages to process
#[instrument(skip_all)]
async fn process_queued_messages(
    notify_url: &Url,
    postgres: &PgPool,
    relay_client: Arc<Client>,
    metrics: Option<&Metrics>,
    analytics: &NotifyAnalytics,
) -> Result<(), NotifyServerError> {
    // Querying for queued messages to be published in a loop until we are done
    loop {
        let result = pick_subscriber_notification_for_processing(postgres, metrics).await?;
        if let Some(notification) = result {
            let notification_id = notification.subscriber_notification;
            info!("Got a notification with id: {}", notification_id);

            let notification_created_at = notification.notification_created_at;
            let process_result = timeout(
                PUBLISHING_TIMEOUT,
                process_notification(
                    notification,
                    notify_url,
                    relay_client.as_ref(),
                    metrics,
                    analytics,
                ),
            )
            .await;

            let handle_error = || {
                update_message_status_queued_or_failed(
                    notification_id,
                    notification_created_at,
                    PUBLISHING_GIVE_UP_TIMEOUT,
                    postgres,
                    metrics,
                )
            };

            match process_result {
                Ok(Ok(())) => {
                    update_message_processing_status(
                        notification_id,
                        SubscriberNotificationStatus::Published,
                        postgres,
                        metrics,
                    )
                    .await?;
                }
                Ok(Err(e)) => {
                    warn!("Error on `process_notification`: {:?}", e);
                    handle_error().await?;
                }
                Err(e) => {
                    warn!("Elapsed error on `process_notification`: {:?}", e);
                    handle_error().await?;
                }
            }
        } else {
            info!("No more notifications to process, stopping the loop");
            break;
        }
    }
    Ok(())
}

#[derive(Debug, Error)]
enum ProcessNotificationError {
    #[error("Decode key: {0}")]
    DecodeKey(DecodeKeyError),

    #[error("Sign JWT: {0}")]
    SignJwt(SignJwtError),

    #[error("Envelope serialization: {0}")]
    EnvelopeSerialization(serde_json::error::Error),

    #[error("Envelope encryption: {0}")]
    EnvelopeEncryption(chacha20poly1305::aead::Error),

    #[error("Relay publish: {0}")]
    RelayPublish(relay_client::error::Error<relay_rpc::rpc::PublishError>),
}

#[instrument(skip_all, fields(notification = ?notification))]
async fn process_notification(
    notification: NotificationToProcess,
    notify_url: &Url,
    relay_client: &Client,
    metrics: Option<&Metrics>,
    analytics: &NotifyAnalytics,
) -> Result<(), ProcessNotificationError> {
    let project_signing_details = {
        let private_key = SigningKey::from_bytes(
            &decode_key(&notification.project_authentication_private_key)
                .map_err(ProcessNotificationError::DecodeKey)?,
        );
        let decoded_client_id = DecodedClientId(
            decode_key(&notification.project_authentication_public_key)
                .map_err(ProcessNotificationError::DecodeKey)?,
        );
        ProjectSigningDetails {
            decoded_client_id,
            private_key,
            app: DidWeb::from_domain(notification.project_app_domain),
        }
    };

    let message = JsonRpcRequest::new(
        NOTIFY_MESSAGE_METHOD,
        NotifyMessageAuth {
            message_auth: sign_message(
                Arc::new(JwtNotification {
                    id: notification.subscriber_notification,
                    sent_at: notification.notification_created_at.timestamp_millis(),
                    r#type: notification.notification_type,
                    title: notification.notification_title.clone(),
                    body: notification.notification_body.clone(),
                    icon: notification.notification_icon.unwrap_or_default(),
                    url: notification
                        .notification_url
                        .map(|_link| {
                            format_follow_link(notify_url, &notification.subscriber_notification)
                                .to_string()
                        })
                        .unwrap_or_default(),
                    is_read: notification.subscriber_notification_is_read,
                }),
                notification.subscriber_account.clone(),
                &project_signing_details,
            )
            .map_err(ProcessNotificationError::SignJwt)?
            .to_string(),
        },
    );

    let sym_key = decode_key(&notification.subscriber_sym_key)
        .map_err(ProcessNotificationError::DecodeKey)?;
    let envelope = Envelope::<EnvelopeType0>::new(
        &sym_key,
        serde_json::to_vec(&message).map_err(ProcessNotificationError::EnvelopeSerialization)?,
    )
    .map_err(ProcessNotificationError::EnvelopeEncryption)?;
    let base64_notification = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());
    let topic = topic_from_key(&sym_key);

    let publish = Publish {
        topic,
        message: base64_notification.into(),
        ttl_secs: NOTIFY_MESSAGE_TTL.as_secs() as u32,
        tag: NOTIFY_MESSAGE_TAG,
        prompt: true,
    };
    let message_id = publish.msg_id();
    publish_relay_message(relay_client, &publish, None, None, metrics, analytics)
        .await
        .map_err(ProcessNotificationError::RelayPublish)?;

    analytics.subscriber_notification(SubscriberNotificationParams {
        project_pk: notification.project,
        project_id: notification.project_project_id,
        subscriber_pk: notification.subscriber,
        account: notification.subscriber_account,
        subscriber_notification_pk: notification.subscriber_notification,
        notification_pk: notification.notification,
        notification_type: notification.notification_type,
        notify_topic: notification.subscriber_topic,
        message_id: message_id.into(),
    });

    Ok(())
}

/// Updates message status back to `Queued` or mark as `Failed`
/// depending on the `giveup_threshold` messsage creation time
#[instrument(skip(postgres, metrics))]
async fn update_message_status_queued_or_failed(
    notification_id: uuid::Uuid,
    notification_created_at: DateTime<Utc>,
    giveup_threshold: Duration,
    postgres: &PgPool,
    metrics: Option<&Metrics>,
) -> Result<(), NotifyServerError> {
    if dead_letter_give_up_check(notification_created_at, giveup_threshold) {
        error!("Message was not processed during the giving up threshold, marking it as failed");
        update_message_processing_status(
            notification_id,
            SubscriberNotificationStatus::Failed,
            postgres,
            metrics,
        )
        .await?;
    } else {
        update_message_processing_status(
            notification_id,
            SubscriberNotificationStatus::Queued,
            postgres,
            metrics,
        )
        .await?;
    }

    Ok(())
}
