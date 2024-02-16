use {
    self::helpers::{pick_subscriber_notification_for_processing, NotificationToProcess},
    crate::{
        analytics::{subscriber_notification::SubscriberNotificationParams, NotifyAnalytics},
        error::NotifyServerError,
        metrics::Metrics,
        notify_message::{sign_message, JwtNotification, ProjectSigningDetails},
        publish_relay_message::publish_relay_message,
        rpc::{decode_key, JsonRpcRequest, NotifyMessageAuth},
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
    tokio::time::{interval, timeout},
    tracing::{error, info, instrument, warn},
    types::SubscriberNotificationStatus,
    wc::metrics::otel::Context,
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

#[instrument(skip_all)]
pub async fn start(
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

    let spawned_tasks_counter = Arc::new(AtomicUsize::new(0));

    // Spawning initial workers to process messages from the queue in case
    // if the service was down before and we need to clear the queue
    for _ in 0..START_WORKERS {
        // Spawning a new task to process the messages from the queue
        tokio::spawn({
            let postgres = postgres.clone();
            let relay_client = relay_client.clone();
            let spawned_tasks_counter = spawned_tasks_counter.clone();
            let metrics = metrics.clone();
            let analytics = analytics.clone();
            async move {
                process_and_handle(
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
                let relay_client = relay_client.clone();
                let spawned_tasks_counter = spawned_tasks_counter.clone();
                let metrics = metrics.clone();
                let analytics = analytics.clone();
                async move {
                    process_and_handle(
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
    postgres: &PgPool,
    relay_client: Arc<Client>,
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

    if let Err(e) = process_queued_messages(postgres, relay_client, metrics, analytics).await {
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
            let process_result = process_with_timeout(
                PUBLISHING_TIMEOUT,
                notification,
                relay_client.clone(),
                metrics,
                analytics,
            );

            match process_result.await {
                Ok(()) => {
                    update_message_processing_status(
                        notification_id,
                        SubscriberNotificationStatus::Published,
                        postgres,
                        metrics,
                    )
                    .await?;
                }
                Err(e) => {
                    warn!("Error on `process_notification`: {:?}", e);
                    update_message_status_queued_or_failed(
                        notification_id,
                        notification_created_at,
                        PUBLISHING_GIVE_UP_TIMEOUT,
                        postgres,
                        metrics,
                    )
                    .await?;
                }
            }
        } else {
            info!("No more notifications to process, stopping the loop");
            break;
        }
    }
    Ok(())
}

/// Process publishing with the threshold timeout
#[instrument(skip(relay_client, metrics, analytics, notification))]
async fn process_with_timeout(
    execution_threshold: Duration,
    notification: NotificationToProcess,
    relay_client: Arc<Client>,
    metrics: Option<&Metrics>,
    analytics: &NotifyAnalytics,
) -> Result<(), NotifyServerError> {
    match timeout(
        execution_threshold,
        process_notification(notification, relay_client.clone(), metrics, analytics),
    )
    .await
    {
        Ok(result) => {
            result?;
        }
        Err(e) => {
            return Err(NotifyServerError::TokioTimeElapsed(e));
        }
    };
    Ok(())
}

#[instrument(skip_all, fields(notification = ?notification))]
async fn process_notification(
    notification: NotificationToProcess,
    relay_client: Arc<Client>,
    metrics: Option<&Metrics>,
    analytics: &NotifyAnalytics,
) -> Result<(), NotifyServerError> {
    let project_signing_details = {
        let private_key = SigningKey::from_bytes(&decode_key(
            &notification.project_authentication_private_key,
        )?);
        let decoded_client_id =
            DecodedClientId(decode_key(&notification.project_authentication_public_key)?);
        ProjectSigningDetails {
            decoded_client_id,
            private_key,
            app: notification.project_app_domain.clone().into(),
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
                    url: notification.notification_url.unwrap_or_default(),
                }),
                notification.subscriber_account.clone(),
                &project_signing_details,
            )?
            .to_string(),
        },
    );

    let sym_key = decode_key(&notification.subscriber_sym_key)?;
    let envelope = Envelope::<EnvelopeType0>::new(&sym_key, &message)?;
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
    publish_relay_message(&relay_client, &publish, metrics).await?;

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
