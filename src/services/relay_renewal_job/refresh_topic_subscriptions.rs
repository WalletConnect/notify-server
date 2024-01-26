use {
    crate::{
        error::NotifyServerError,
        metrics::Metrics,
        model::helpers::{get_project_topics, get_subscriber_topics},
    },
    futures_util::{StreamExt, TryFutureExt, TryStreamExt},
    relay_client::http::Client,
    relay_rpc::{domain::Topic, rpc::MAX_SUBSCRIPTION_BATCH_SIZE},
    sqlx::PgPool,
    std::time::Instant,
    tracing::{info, instrument},
    wc::metrics::otel::Context,
};

// TODO change error type
#[instrument(skip_all)]
pub async fn run(
    key_agreement_topic: Topic,
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

    // Collect each batch into its own vec, since `batch_subscribe` would convert
    // them anyway.
    let topics = topics
        .chunks(MAX_SUBSCRIPTION_BATCH_SIZE)
        .map(|chunk| chunk.to_vec())
        .collect::<Vec<_>>();

    // Limit concurrency to avoid overwhelming the relay with requests.
    const REQUEST_CONCURRENCY: usize = 200;

    futures_util::stream::iter(topics)
        .map(|topics| {
            // Map result to an unsized type to avoid allocation when collecting,
            // as we don't care about subscription IDs.
            client.batch_subscribe_blocking(topics).map_ok(|_| ())
        })
        .buffer_unordered(REQUEST_CONCURRENCY)
        .try_collect::<Vec<_>>()
        .await?;

    let elapsed: u64 = start
        .elapsed()
        .as_millis()
        .try_into()
        .expect("No error getting ms of elapsed time");
    info!("resubscribe took {elapsed}ms");

    if let Some(metrics) = metrics {
        let ctx = Context::current();
        metrics
            .subscribed_project_topics
            .observe(&ctx, project_topics_count as u64, &[]);
        metrics
            .subscribed_subscriber_topics
            .observe(&ctx, subscriber_topics_count as u64, &[]);
        metrics.subscribe_latency.record(&ctx, elapsed, &[]);
    }

    Ok(())
}
