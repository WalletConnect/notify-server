use crate::{state::AppState, types::WatchSubscriptionsEntry};
use mongodb::bson::doc;
use std::{sync::Arc, time::Duration};
use tracing::info;

pub async fn watcher_expiration_job(state: Arc<AppState>) {
    let timer = tokio::time::interval(Duration::from_secs(5));

    loop {
        info!("running expiry job");
        if let Err(e) = job(state.as_ref()).await {

        }
    }
}

async fn job(state: &AppState) -> mongodb::error::Result<mongodb::results::DeleteResult> {
    let delete_result = state
        .database
        .collection::<WatchSubscriptionsEntry>("watch_subscriptions")
        .delete_many(doc! { "did_key": &did_key }, None)
        .await?;
    info!("deleted_count: {}", delete_result.deleted_count);
}
