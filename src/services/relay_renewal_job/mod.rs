use {
    crate::{error::NotifyServerError, metrics::Metrics},
    chrono::Duration,
    relay_client::http::Client,
    relay_rpc::{auth::ed25519_dalek::SigningKey, domain::Topic},
    sqlx::PgPool,
    std::{future::Future, sync::Arc},
    tokio::{sync::Mutex, time},
    tracing::{error, info, instrument},
    url::Url,
};

mod refresh_topic_subscriptions;
mod register_webhook;

pub async fn start(
    key_agreement_topic: Topic,
    notify_url: Url,
    keypair: SigningKey,
    relay_client: Arc<Client>,
    postgres: PgPool,
    metrics: Option<Metrics>,
) -> Result<impl Future<Output = ()>, NotifyServerError> {
    let period = Duration::days(1);

    let mut interval = time::interval(period.to_std().expect("Should be able to convert to STD"));

    let renew_all_topics_lock = Arc::new(Mutex::new(false));

    // We must be able to run the job once on startup or we are non-functional
    // Call tick() now so that the first tick() inside the loop actually waits for the period
    interval.tick().await;
    job(
        key_agreement_topic.clone(),
        renew_all_topics_lock.clone(),
        &notify_url,
        &keypair,
        &relay_client,
        &postgres,
        metrics.as_ref(),
    )
    .await?;

    Ok(async move {
        loop {
            interval.tick().await;
            info!("Running relay renewal job");
            if let Err(e) = job(
                key_agreement_topic.clone(),
                renew_all_topics_lock.clone(),
                &notify_url,
                &keypair,
                &relay_client,
                &postgres,
                metrics.as_ref(),
            )
            .await
            {
                error!("Error running relay renewal job: {e:?}");
                // TODO metrics
            }
        }
    })
}

#[instrument(skip_all)]
async fn job(
    key_agreement_topic: Topic,
    renew_all_topics_lock: Arc<Mutex<bool>>,
    notify_url: &Url,
    keypair: &SigningKey,
    relay_client: &Client,
    postgres: &PgPool,
    metrics: Option<&Metrics>,
) -> Result<(), NotifyServerError> {
    register_webhook::run(notify_url, keypair, relay_client).await?;
    refresh_topic_subscriptions::run(
        key_agreement_topic,
        renew_all_topics_lock,
        relay_client,
        postgres,
        metrics,
    )
    .await?;
    Ok(())
}
