use {
    crate::{
        error::NotifyServerError,
        metrics::Metrics,
        model::helpers::{get_project_topics, get_subscriber_topics},
        publish_relay_message::batch_subscribe_relay_topics,
    },
    futures_util::StreamExt,
    relay_client::http::Client,
    relay_rpc::domain::Topic,
    sqlx::PgPool,
    std::{sync::Arc, time::Instant},
    tokio::sync::Mutex,
    tracing::{error, info, instrument},
};

// TODO change error type
#[instrument(skip_all)]
pub async fn run(
    key_agreement_topic: Topic,
    renew_all_topics_lock: Arc<Mutex<bool>>,
    client: &Client,
    postgres: &PgPool,
    metrics: Option<&Metrics>,
) -> Result<(), NotifyServerError> {
    // TODO only renew when the subscription needs it
    info!("Resubscribing to all topics");
    let start = Instant::now();

    let subscriber_topics = get_subscriber_topics(postgres, metrics).await?;
    let subscriber_topics_count = subscriber_topics.len();
    info!("subscriber_topics_count: {subscriber_topics_count}");

    let project_topics = get_project_topics(postgres, metrics).await?;
    let project_topics_count = project_topics.len();
    info!("project_topics_count: {project_topics_count}");

    // TODO: These need to be paginated and streamed from the database directly
    // instead of collecting them to a single giant vec.
    let topics = [key_agreement_topic]
        .into_iter()
        .chain(subscriber_topics.into_iter())
        .chain(project_topics.into_iter())
        .collect::<Vec<_>>();
    let topics_count = topics.len();
    info!("topics_count: {topics_count}");

    let topic_batches = topics
        // Chunk as 1 since we don't yet have the ability to process each topic error individually
        // https://github.com/WalletConnect/notify-server/issues/395
        // .chunks(MAX_SUBSCRIPTION_BATCH_SIZE)
        .chunks(1)
        .map(|topics| topics.to_vec())
        .collect::<Vec<_>>();

    // Limit concurrency to avoid overwhelming the relay with requests.
    const REQUEST_CONCURRENCY: usize = 25;

    // If operation already running, don't start another one
    let mut operation_running = renew_all_topics_lock.lock().await;
    if !*operation_running {
        info!("Starting renew operation");
        *operation_running = true;
        // Renew all subscription TTLs.
        // This can take a long time (e.g. 2 hours), so cannot block server startup.
        tokio::task::spawn({
            let client = client.clone();
            let metrics = metrics.cloned();
            let renew_all_topics_lock = renew_all_topics_lock.clone();
            async move {
                let client = &client;
                let metrics = metrics.as_ref();

                let result = futures_util::stream::iter(topic_batches)
                    .map(|topics| batch_subscribe_relay_topics(client, topics, metrics))
                    .buffer_unordered(REQUEST_CONCURRENCY)
                    .collect::<Vec<_>>()
                    .await;
                let elapsed: u64 = start.elapsed().as_millis().try_into().unwrap();
                for result in &result {
                    if let Err(e) = result {
                        // An error here is bad, as topics will not have been renewed.
                        // However, this should be rare and many resubscribes will happen within 30 days so all topics should be renewed eventually.
                        // With <https://github.com/WalletConnect/notify-server/issues/325> we will be able to guarantee renewal much better.
                        error!("Failed to renew some topic subscriptions: {e}");
                    }
                }
                info!("Completed topic renew job (possibly with errors) in {elapsed}ms");
                *renew_all_topics_lock.lock().await = false;

                if let Some(metrics) = metrics {
                    metrics
                        .subscribed_project_topics
                        .observe(project_topics_count as u64, &[]);
                    metrics
                        .subscribed_subscriber_topics
                        .observe(subscriber_topics_count as u64, &[]);
                    metrics.subscribe_latency.record(elapsed, &[]);
                }
            }
        });
    } else {
        info!("Renew operation already running");
    }

    Ok(())
}
