use {
    crate::{state::AppState, types::WatchSubscriptionsEntry},
    chrono::Utc,
    mongodb::bson::doc,
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

async fn job(state: &AppState) -> mongodb::error::Result<()> {
    let now = Utc::now().timestamp();
    let delete_result = state
        .database
        .collection::<WatchSubscriptionsEntry>("watch_subscriptions")
        .delete_many(doc! { "expiry": { "$lte": now } }, None)
        .await?;
    let count = delete_result.deleted_count;
    info!("expired watchers: {count}");
    Ok(())
}
