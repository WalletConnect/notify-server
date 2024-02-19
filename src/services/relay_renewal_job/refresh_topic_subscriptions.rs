use {
    crate::{
        error::NotifyServerError,
        metrics::Metrics,
        model::helpers::{get_project_topics, get_subscriber_topics},
        publish_relay_message::{extend_subscription_ttl, subscribe_relay_topic},
    },
    futures_util::{StreamExt, TryFutureExt, TryStreamExt},
    relay_client::http::Client,
    relay_rpc::domain::Topic,
    sqlx::PgPool,
    std::{sync::Arc, time::Instant},
    tokio::sync::Mutex,
    tracing::{error, info, instrument},
    wc::metrics::otel::Context,
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

                // Using `batch_subscription` was removed in https://github.com/WalletConnect/notify-server/pull/359
                // We can't really use this right now because we are also extending the topic TTL which could take longer than the 5m TTL
                let result = futures_util::stream::iter(topics)
                    .map(|topic| async move {
                        // Subscribe a second time as the initial subscription above may have expired
                        subscribe_relay_topic(client, &topic, metrics)
                            .map_ok(|_| ())
                            .map_err(NotifyServerError::from)
                            .and_then(|_| {
                                // Subscribing only guarantees 5m TTL, so we always need to extend it.
                                extend_subscription_ttl(client, topic.clone(), metrics)
                                    .map_ok(|_| ())
                                    .map_err(Into::into)
                            })
                            .await
                    })
                    // Above we want to resubscribe as quickly as possible so use a high concurrency value
                    // But here we prefer stability and are OK with a lower value
                    .buffer_unordered(REQUEST_CONCURRENCY)
                    .try_collect::<Vec<_>>()
                    .await;
                let elapsed: u64 = start.elapsed().as_millis().try_into().unwrap();
                if let Err(e) = result {
                    // An error here is bad, as topics will not have been renewed.
                    // However, this should be rare and many resubscribes will happen within 30 days so all topics should be renewed eventually.
                    // With <https://github.com/WalletConnect/notify-server/issues/325> we will be able to guarantee renewal much better.
                    error!("Failed to renew all topic subscriptions in {elapsed}ms: {e}");
                } else {
                    info!("Success renewing all topic subscriptions in {elapsed}ms");
                }
                *renew_all_topics_lock.lock().await = false;

                if let Some(metrics) = metrics {
                    let ctx = Context::current();
                    metrics.subscribed_project_topics.observe(
                        &ctx,
                        project_topics_count as u64,
                        &[],
                    );
                    metrics.subscribed_subscriber_topics.observe(
                        &ctx,
                        subscriber_topics_count as u64,
                        &[],
                    );
                    metrics.subscribe_latency.record(&ctx, elapsed, &[]);
                }
            }
        });
    } else {
        info!("Renew operation already running");
    }

    Ok(())
}
