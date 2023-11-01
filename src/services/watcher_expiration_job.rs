use {
    crate::model::helpers::delete_expired_subscription_watchers,
    sqlx::PgPool,
    std::time::Duration,
    tokio::time,
    tracing::{info, warn},
};

pub async fn start(postgres: PgPool) {
    let mut interval = time::interval(Duration::from_secs(60 * 60));

    loop {
        interval.tick().await;
        info!("Running watcher_expiration_job");
        if let Err(e) = job(&postgres).await {
            warn!("Error running watcher_expiration_job: {e:?}");
        }
    }
}

async fn job(postgres: &PgPool) -> sqlx::Result<()> {
    let count = delete_expired_subscription_watchers(postgres).await?;
    info!("Expired {count} watchers");
    Ok(())
}
