use {
    crate::{model::helpers::delete_expired_subscription_watchers, state::AppState},
    std::{sync::Arc, time::Duration},
    tracing::{info, warn},
};

pub async fn watcher_expiration_job(state: Arc<AppState>) {
    let mut interval = tokio::time::interval(Duration::from_secs(5 * 60));

    loop {
        interval.tick().await;
        info!("running expiry job");
        if let Err(e) = job(state.as_ref()).await {
            warn!("Error expiring watchers: {e} {e:?}");
        }
    }
}

async fn job(state: &AppState) -> sqlx::Result<()> {
    let count = delete_expired_subscription_watchers(&state.postgres).await?;
    info!("expired watchers: {count}");
    Ok(())
}
