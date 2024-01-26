use {
    crate::{metrics::Metrics, model::helpers::delete_expired_subscription_watchers},
    sqlx::PgPool,
    std::time::Duration,
    tokio::time,
    tracing::{error, info},
};

pub async fn start(postgres: PgPool, metrics: Option<Metrics>) {
    let mut interval = time::interval(Duration::from_secs(60 * 60));

    loop {
        interval.tick().await;
        info!("Running watcher expiration job");
        if let Err(e) = job(&postgres, metrics.as_ref()).await {
            error!("Error running watcher expiration job: {e:?}");
            // TODO metrics
        }
    }
}

async fn job(postgres: &PgPool, metrics: Option<&Metrics>) -> sqlx::Result<()> {
    let count = delete_expired_subscription_watchers(postgres, metrics).await?;
    info!("Expired {count} watchers");
    Ok(())
}
