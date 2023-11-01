use {
    crate::model::helpers::delete_expired_subscription_watchers,
    sqlx::PgPool,
    std::time::Duration,
    tracing::{info, warn},
};

pub async fn watcher_expiration_job(postgres: PgPool) {
    let mut interval = tokio::time::interval(Duration::from_secs(5 * 60));

    loop {
        interval.tick().await;
        info!("running expiry job");
        if let Err(e) = job(&postgres).await {
            warn!("Error expiring watchers: {e} {e:?}");
        }
    }
}

async fn job(postgres: &PgPool) -> sqlx::Result<()> {
    let count = delete_expired_subscription_watchers(postgres).await?;
    info!("expired watchers: {count}");
    Ok(())
}
