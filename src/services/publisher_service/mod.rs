use {
    self::helpers::{pick_subscriber_notification_for_processing, NotificationToProcess},
    crate::{
        analytics::{subscriber_notification::SubscriberNotificationParams, NotifyAnalytics},
        jsonrpc::{JsonRpcParams, JsonRpcPayload, NotifyPayload},
        metrics::Metrics,
        notify_message::{sign_message, JwtNotification, ProjectSigningDetails},
        publish_relay_message::publish_relay_message,
        services::websocket_server::decode_key,
        spec::{NOTIFY_MESSAGE_TAG, NOTIFY_MESSAGE_TTL},
        types::{Envelope, EnvelopeType0},
    },
    base64::Engine,
    helpers::update_message_processing_status,
    relay_client::http::Client,
    relay_rpc::{
        domain::{ClientId, DecodedClientId, Topic},
        rpc::{msg_id::MsgId, Publish, JSON_RPC_VERSION_STR},
    },
    sqlx::{postgres::PgListener, PgPool},
    std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        time::Duration,
    },
    tokio::time::{interval, timeout},
    tracing::{info, instrument, warn},
    types::SubscriberNotificationStatus,
    wc::metrics::otel::Context,
};

pub mod helpers;
pub mod types;

// TODO: These should be configurable, add to the config
// Maximum of the parallel messages processing workers
const MAX_WORKERS: usize = 10;
// Number of workers to be spawned on the service start to clean the queue
const START_WORKERS: usize = 10;
// Messages queue stats observing database polling interval
const QUEUE_STATS_POLLING_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);
// Maximum publishing time in minutes before the publish will be considered as failed
// and the messages in queue with the `processing` state will be returned to the queue
const PUBLISHING_TIMEOUT_MINUTES: i8 = 5;
// Interval in seconds to check for dead letters
const DEAD_LETTER_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

#[instrument(skip_all)]
pub async fn start(
    postgres: PgPool,
    relay_http_client: Arc<Client>,
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
        async move {
            let mut poll_interval = interval(DEAD_LETTER_POLL_INTERVAL);
            loop {
                poll_interval.tick().await;
                if let Err(e) =
                    helpers::dead_letters_check(PUBLISHING_TIMEOUT_MINUTES, &postgres).await
                {
                    warn!("Error on dead letters check: {:?}", e);
                }
            }
        }
    });

    // TODO: Spawned tasks counter should be exported to metrics
    let spawned_tasks_counter = Arc::new(AtomicUsize::new(0));

    // Spawning initial workers to process messages from the queue in case
    // if the service was down before and we need to clear the queue
    for _ in 0..START_WORKERS {
        // Spawning a new task to process the messages from the queue
        tokio::spawn({
            let postgres = postgres.clone();
            let relay_http_client = relay_http_client.clone();
            let spawned_tasks_counter = spawned_tasks_counter.clone();
            let metrics = metrics.clone();
            let analytics = analytics.clone();
            async move {
                process_and_handle(
                    &postgres,
                    relay_http_client,
                    metrics.as_ref(),
                    &analytics,
                    spawned_tasks_counter,
                )
                .await;
            }
        });
    }

    let mut pg_notify_listener = PgListener::connect_with(&postgres).await?;
    pg_notify_listener
        .listen("notification_for_delivery")
        .await?;

    loop {
        // Blocking waiting for the notification of the new message in a queue
        pg_notify_listener.recv().await?;

        if spawned_tasks_counter.load(Ordering::SeqCst) < MAX_WORKERS {
            // Spawning a new task to process the messages from the queue
            tokio::spawn({
                let postgres = postgres.clone();
                let relay_http_client = relay_http_client.clone();
                let spawned_tasks_counter = spawned_tasks_counter.clone();
                let metrics = metrics.clone();
                let analytics = analytics.clone();
                async move {
                    process_and_handle(
                        &postgres,
                        relay_http_client,
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
    postgres: &PgPool,
    relay_http_client: Arc<Client>,
    metrics: Option<&Metrics>,
    analytics: &NotifyAnalytics,
    spawned_tasks_counter: Arc<AtomicUsize>,
) {
    spawned_tasks_counter.fetch_add(1, Ordering::SeqCst);

    let ctx = Context::current();
    if let Some(metrics) = metrics {
        metrics.publishing_workers_count.observe(
            &ctx,
            spawned_tasks_counter.load(Ordering::SeqCst) as u64,
            &[],
        );
        // TODO: Add worker execution time metric
    }

    if let Err(e) = process_queued_messages(postgres, relay_http_client, metrics, analytics).await {
        if let Some(metrics) = metrics {
            metrics.publishing_workers_errors.add(&ctx, 1, &[]);
        }
        warn!("Error on processing queued messages by the worker: {:?}", e);
    }

    spawned_tasks_counter.fetch_sub(1, Ordering::SeqCst);

    if let Some(metrics) = metrics {
        metrics.publishing_workers_count.observe(
            &ctx,
            spawned_tasks_counter.load(Ordering::SeqCst) as u64,
            &[],
        );
    }
}

/// Picking messages from the queue and processing them in a loop until
/// there are no more messages to process
#[instrument(skip_all)]
async fn process_queued_messages(
    postgres: &PgPool,
    relay_http_client: Arc<Client>,
    metrics: Option<&Metrics>,
    analytics: &NotifyAnalytics,
) -> crate::error::Result<()> {
    // Querying for queued messages to be published in a loop until we are done
    loop {
        let result = pick_subscriber_notification_for_processing(postgres).await?;
        if let Some(notification) = result {
            let notification_id = notification.subscriber_notification;
            info!("Got a notification with id: {}", notification_id);

            update_message_processing_status(
                notification_id,
                SubscriberNotificationStatus::Published,
                postgres,
                metrics,
            )
            .await?;
            let process_result = process_with_timeout(
                Duration::from_secs(PUBLISHING_TIMEOUT_MINUTES as u64 * 60),
                notification,
                relay_http_client.clone(),
                metrics,
                analytics,
            );

            match process_result.await {
                Ok(_) => {
                    update_message_processing_status(
                        notification_id,
                        SubscriberNotificationStatus::Published,
                        postgres,
                    )
                    .await?;
                }
                Err(e) => match e {
                    crate::error::Error::TokioTimeElapsed(e) => {
                        warn!("Timeout elapsed on publishing to the relay: {:?}", e);
                        update_message_processing_status(
                            notification_id,
                            SubscriberNotificationStatus::Queued,
                            postgres,
                        )
                        .await?;
                    }
                    crate::error::Error::RelayPublishingError(e) => {
                        warn!("RelayPublishingError on `process_notification`: {:?}", e);
                        update_message_processing_status(
                            notification_id,
                            SubscriberNotificationStatus::Failed,
                            postgres,
                        )
                        .await?;
                    }
                    e => {
                        warn!("Unknown error on `process_notification`: {:?}", e);
                        update_message_processing_status(
                            notification_id,
                            SubscriberNotificationStatus::Failed,
                            postgres,
                        )
                        .await?;
                    }
                },
            }
        } else {
            info!("No more notifications to process, stopping the loop");
            break;
        }
    }
    Ok(())
}

/// Process publishing with the threshold timeout
#[instrument(skip(relay_http_client, metrics, analytics, notification))]
async fn process_with_timeout(
    execution_threshold: Duration,
    notification: NotificationToProcess,
    relay_http_client: Arc<Client>,
    metrics: Option<&Metrics>,
    analytics: &NotifyAnalytics,
) -> crate::error::Result<()> {
    match timeout(
        execution_threshold,
        process_notification(notification, relay_http_client.clone(), metrics, analytics),
    )
    .await
    {
        Ok(result) => {
            if let Err(e) = result {
                return Err(crate::error::Error::RelayPublishingError(e.to_string()));
            }
        }
        Err(e) => {
            return Err(crate::error::Error::TokioTimeElapsed(e));
        }
    };
    Ok(())
}

#[instrument(skip_all, fields(notification = ?notification))]
async fn process_notification(
    notification: NotificationToProcess,
    relay_http_client: Arc<Client>,
    metrics: Option<&Metrics>,
    analytics: &NotifyAnalytics,
) -> crate::error::Result<()> {
    let project_signing_details = {
        let private_key = ed25519_dalek::SigningKey::from_bytes(&decode_key(
            &notification.project_authentication_private_key,
        )?);
        let decoded_client_id =
            DecodedClientId(decode_key(&notification.project_authentication_public_key)?);
        let identity = ClientId::from(decoded_client_id);
        ProjectSigningDetails {
            identity,
            private_key,
            app: notification.project_app_domain.clone().into(),
        }
    };

    let id = chrono::Utc::now().timestamp_millis().unsigned_abs();
    let message = JsonRpcPayload {
        id,
        jsonrpc: JSON_RPC_VERSION_STR.to_owned(),
        params: JsonRpcParams::Push(NotifyPayload {
            message_auth: sign_message(
                Arc::new(JwtNotification {
                    id: notification.subscriber_notification,
                    r#type: notification.notification_type,
                    title: notification.notification_title.clone(),
                    body: notification.notification_body.clone(),
                    icon: notification.notification_icon.unwrap_or_default(),
                    url: notification.notification_url.unwrap_or_default(),
                }),
                notification.subscriber_account.clone(),
                &project_signing_details,
            )?
            .to_string(),
        }),
    };

    let sym_key = decode_key(&notification.subscriber_sym_key)?;
    let envelope = Envelope::<EnvelopeType0>::new(&sym_key, &message)?;
    let base64_notification = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());
    let topic = Topic::new(sha256::digest(&sym_key).into());

    let publish = Publish {
        topic,
        message: base64_notification.into(),
        ttl_secs: NOTIFY_MESSAGE_TTL.as_secs() as u32,
        tag: NOTIFY_MESSAGE_TAG,
        prompt: true,
    };
    let message_id = publish.msg_id();
    publish_relay_message(&relay_http_client, &publish, metrics).await?;

    analytics.message(SubscriberNotificationParams {
        project_pk: notification.project,
        project_id: notification.project_project_id,
        subscriber_pk: notification.subscriber,
        account: notification.subscriber_account,
        notification_type: notification.notification_type,
        notify_topic: notification.subscriber_topic,
        message_id: message_id.into(),
    });

    Ok(())
}
