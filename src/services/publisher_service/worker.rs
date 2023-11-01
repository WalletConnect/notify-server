use {
    super::{
        helpers::{
            get_notification_by_notification_id, get_subscriber_by_subscriber_id, sign_message,
            update_message_processing_status,
        },
        types::{NotificationStates, ProcessingMessageItem, ProjectSigningDetails},
    },
    crate::{
        jsonrpc::{JsonRpcParams, JsonRpcPayload, NotifyPayload},
        model::helpers::get_project_by_project_id,
        services::websocket_service::decode_key,
        spec::{NOTIFY_MESSAGE_TAG, NOTIFY_MESSAGE_TTL},
        types::{Envelope, EnvelopeType0},
    },
    base64::Engine,
    relay_rpc::{
        domain::{ClientId, DecodedClientId, Topic},
        rpc::Publish,
    },
    sqlx::{postgres::PgListener, PgPool, Postgres},
    std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        time::Duration,
    },
    tracing::{info, instrument, warn},
};

// TODO: These should be configurable, add to the config
// Maximum of the parallel messages processing workers
const MAX_WORKERS: usize = 10;
// Number of workers to be spawned on the service start to clean the queue
const START_WORKERS: usize = 10;

#[instrument]
pub async fn run(
    pg_pool: &Arc<PgPool>,
    relay_client: &Arc<relay_client::http::Client>,
) -> Result<(), sqlx::Error> {
    let mut pg_notify_listener = PgListener::connect_with(pg_pool)
        .await
        .expect("Notifying listener failed to connect to Postgres");
    pg_notify_listener
        .listen("notification_for_delivery")
        .await
        .expect("Failed to listen to Postgres events channels");

    // TODO: Spawned tasks counter should be exported to metrics
    let spawned_tasks_counter = Arc::new(AtomicUsize::new(0));

    // Spawning initial workers to process messages from the queue in case
    // if the service was down before and we need to clear the queue
    for _ in 0..START_WORKERS {
        // Spawning a new task to process the messages from the queue
        tokio::spawn({
            let pg_pool = pg_pool.clone();
            let relay_client = relay_client.clone();
            let spawned_tasks_counter = spawned_tasks_counter.clone();
            async move { process_queued_messages(&spawned_tasks_counter, &pg_pool, &relay_client).await }
        });
    }

    loop {
        // Blocking waiting for the notification of the new message in a queue
        pg_notify_listener.recv().await?;

        if spawned_tasks_counter.load(Ordering::SeqCst) < MAX_WORKERS {
            // Spawning a new task to process the messages from the queue
            tokio::spawn({
                let pg_pool = pg_pool.clone();
                let relay_client = relay_client.clone();
                let spawned_tasks_counter = spawned_tasks_counter.clone();
                async move {
                    process_queued_messages(&spawned_tasks_counter, &pg_pool, &relay_client).await
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
#[instrument]
async fn process_queued_messages(
    spawned_tasks_counter: &Arc<AtomicUsize>,
    pg_pool: &PgPool,
    relay_client: &Arc<relay_client::http::Client>,
) {
    // Increasing the number of spawned tasks atomic counter
    spawned_tasks_counter.fetch_add(1, Ordering::SeqCst);

    // Querying for queued messages to be published in a loop until we are done
    loop {
        match pick_message_for_processing(pg_pool).await {
            Ok(result) => {
                if let Some(message_to_process) = result {
                    info!("Got a message with id: {}", message_to_process.id);
                    process_message(
                        &message_to_process.project,
                        &message_to_process.notification,
                        &message_to_process.subscriber,
                        pg_pool,
                        relay_client.clone(),
                    )
                    .await
                    .unwrap();

                    update_message_processing_status(
                        &message_to_process.id,
                        NotificationStates::Published,
                        pg_pool,
                    )
                    .await
                    .unwrap();
                } else {
                    info!("No more messages to process, stopping the loop");
                    break;
                }
            }
            Err(_) => {
                // TODO: process this error properly
                // Decreasing the number of spawned tasks atomic counter
                spawned_tasks_counter.fetch_sub(1, Ordering::SeqCst);
                return;
            }
        }
    }

    // Decreasing the number of spawned tasks atomic counter
    spawned_tasks_counter.fetch_sub(1, Ordering::SeqCst);
}

#[instrument]
async fn pick_message_for_processing(
    pg_pool: &PgPool,
) -> Result<Option<ProcessingMessageItem>, sqlx::Error> {
    // Getting the message to be published from the `subscriber_notification`,
    // updating the status to the `processing`,
    // and returning the message to be processed with the project id
    let pick_message_query = "
        WITH cte AS (
            SELECT id
            FROM subscriber_notification
            WHERE status = 'queued'
            LIMIT 1
            FOR UPDATE SKIP LOCKED
        )
        UPDATE subscriber_notification
        SET status = 'processing', updated_at = NOW()
        WHERE id = (SELECT id FROM cte)
        RETURNING
            id, notification, subscriber,
            (SELECT project
                FROM notification
                WHERE id = notification
            ) AS project
    ";
    sqlx::query_as::<Postgres, ProcessingMessageItem>(pick_message_query)
        .fetch_optional(pg_pool)
        .await
}

/// Creates a message to be sent to the relay and calling the relay client
#[instrument]
async fn process_message(
    project_id: &str,
    notification_id: &str,
    account_id: &str,
    pg_pool: &PgPool,
    relay_client: Arc<relay_client::http::Client>,
) -> Result<(), anyhow::Error> {
    let project = get_project_by_project_id(project_id.into(), pg_pool).await?;
    let project_signing_details = {
        let private_key = ed25519_dalek::SigningKey::from_bytes(&decode_key(
            &project.authentication_private_key,
        )?);
        let decoded_client_id = DecodedClientId(
            hex::decode(project.authentication_public_key.clone())?[0..32].try_into()?,
        );
        let identity = ClientId::from(decoded_client_id);
        ProjectSigningDetails {
            identity,
            private_key,
            app: project.app_domain.into(),
        }
    };

    let id = chrono::Utc::now().timestamp_millis().unsigned_abs();
    let notification = get_notification_by_notification_id(notification_id, pg_pool).await?;
    let message = JsonRpcPayload {
        id,
        jsonrpc: "2.0".to_string(),
        params: JsonRpcParams::Push(NotifyPayload {
            message_auth: sign_message(notification, account_id.into(), &project_signing_details)?
                .to_string(),
        }),
    };

    let subscriber = get_subscriber_by_subscriber_id(account_id, pg_pool).await?;
    let sym_key = decode_key(&subscriber.sym_key)?;
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
    do_publish(relay_client.clone(), account_id, &publish)
        .await
        .unwrap();
    Ok(())
}

/// Publishes a notification to the relay using the relay client
#[instrument]
async fn do_publish(
    relay_client: Arc<relay_client::http::Client>,
    account_id: &str,
    publish: &Publish,
) -> Result<(), relay_client::error::Error> {
    let client_publish_call = || {
        relay_client.publish(
            publish.topic.clone(),
            publish.message.clone(),
            publish.tag,
            Duration::from_secs(publish.ttl_secs as u64),
            publish.prompt,
        )
    };

    // Handling retrying attempts on temporary relay errors
    let mut tries = 0;
    while let Err(e) = client_publish_call().await {
        tries += 1;
        if tries >= 10 {
            return Err(e);
        }
        warn!(
            "Temporary error on publishing notification for account {} on topic {}, \
            retrying attempt {} in 1s: {e:?}",
            account_id, publish.topic, tries
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    Ok(())
}
