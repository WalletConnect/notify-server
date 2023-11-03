use {
    self::helpers::{pick_subscriber_notification_for_processing, NotificationToProcess},
    crate::{
        analytics::{subscriber_notification::SubscriberNotificationParams, NotifyAnalytics},
        jsonrpc::{JsonRpcParams, JsonRpcPayload, NotifyPayload},
        notify_message::{sign_message, NotifyNotification, ProjectSigningDetails},
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
        rpc::{msg_id::MsgId, Publish},
    },
    sqlx::{postgres::PgListener, PgPool},
    std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    tracing::{info, instrument, warn},
    types::SubscriberNotificationStatus,
};

pub mod helpers;
pub mod types;

// TODO: These should be configurable, add to the config
// Maximum of the parallel messages processing workers
const MAX_WORKERS: usize = 10;
// Number of workers to be spawned on the service start to clean the queue
const START_WORKERS: usize = 10;

#[instrument(skip_all)]
pub async fn start(
    postgres: PgPool,
    relay_http_client: Arc<Client>,
    analytics: NotifyAnalytics,
) -> Result<(), sqlx::Error> {
    let mut pg_notify_listener = PgListener::connect_with(&postgres).await?;
    pg_notify_listener
        .listen("notification_for_delivery")
        .await?;

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
            let analytics = analytics.clone();
            async move {
                // TODO make DRY with below
                spawned_tasks_counter.fetch_add(1, Ordering::SeqCst);
                if let Err(e) =
                    process_queued_messages(&postgres, relay_http_client, &analytics).await
                {
                    warn!("Error on processing queued messages: {:?}", e);
                }
                spawned_tasks_counter.fetch_sub(1, Ordering::SeqCst);
            }
        });
    }

    loop {
        // Blocking waiting for the notification of the new message in a queue
        pg_notify_listener.recv().await?;

        if spawned_tasks_counter.load(Ordering::SeqCst) < MAX_WORKERS {
            // Spawning a new task to process the messages from the queue
            tokio::spawn({
                let postgres = postgres.clone();
                let relay_http_client = relay_http_client.clone();
                let spawned_tasks_counter = spawned_tasks_counter.clone();
                let analytics = analytics.clone();
                async move {
                    spawned_tasks_counter.fetch_add(1, Ordering::SeqCst);
                    if let Err(e) =
                        process_queued_messages(&postgres, relay_http_client, &analytics).await
                    {
                        warn!("Error on processing queued messages: {:?}", e);
                    }
                    spawned_tasks_counter.fetch_sub(1, Ordering::SeqCst);
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

/// Picking messages from the queue and processing them in a loop until
/// there are no more messages to process
#[instrument(skip_all)]
async fn process_queued_messages(
    postgres: &PgPool,
    relay_http_client: Arc<Client>,
    analytics: &NotifyAnalytics,
) -> crate::error::Result<()> {
    // Querying for queued messages to be published in a loop until we are done
    loop {
        let result = pick_subscriber_notification_for_processing(postgres).await?;
        if let Some(notification) = result {
            let notification_id = notification.id;
            info!("Got a notification with id: {}", notification_id);
            process_notification(notification, relay_http_client.clone(), analytics).await?;

            update_message_processing_status(
                notification_id,
                SubscriberNotificationStatus::Published,
                postgres,
            )
            .await?;
        } else {
            info!("No more notifications to process, stopping the loop");
            break;
        }
    }
    Ok(())
}

#[instrument(skip_all, fields(notification = ?notification))]
async fn process_notification(
    notification: NotificationToProcess,
    relay_http_client: Arc<Client>,
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
        jsonrpc: "2.0".to_string(),
        params: JsonRpcParams::Push(NotifyPayload {
            message_auth: sign_message(
                Arc::new(NotifyNotification {
                    r#type: notification.notification_type.clone(),
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
    publish_relay_message(&relay_http_client, &publish).await?;

    analytics.message(SubscriberNotificationParams {
        project_pk: notification.project,
        project_id: notification.project_project_id,
        subscriber_pk: notification.subscriber,
        account: notification.subscriber_account,
        notification_type: notification.notification_type.into(),
        notify_topic: notification.subscriber_topic,
        message_id: message_id.into(),
    });

    Ok(())
}
