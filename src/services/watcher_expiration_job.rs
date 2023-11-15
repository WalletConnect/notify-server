use {
    crate::model::helpers::delete_expired_subscription_watchers,
    sqlx::PgPool,
    std::time::Duration,
    tokio::time,
    tracing::{error, info},
};

pub async fn start(postgres: PgPool) {
    let mut interval = time::interval(Duration::from_secs(60 * 60));

    loop {
        interval.tick().await;
        info!("Running watcher expiration job");
        if let Err(e) = job(&postgres).await {
            error!("Error running watcher expiration job: {e:?}");
        }
    }
}

async fn job(postgres: &PgPool) -> sqlx::Result<()> {
    let count = delete_expired_subscription_watchers(postgres).await?;
    info!("Expired {count} watchers");
    Ok(())
}
