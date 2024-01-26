use {
    crate::{
        services::public_http_server::RELAY_WEBHOOK_ENDPOINT,
        spec::{
            NOTIFY_DELETE_TAG, NOTIFY_GET_NOTIFICATIONS_TAG, NOTIFY_SUBSCRIBE_TAG,
            NOTIFY_UPDATE_TAG, NOTIFY_WATCH_SUBSCRIPTIONS_TAG,
        },
    },
    relay_client::{
        error::Error,
        http::{Client, WatchRegisterRequest},
    },
    relay_rpc::{
        auth::ed25519_dalek::Keypair,
        rpc::{WatchStatus, WatchType},
    },
    std::time::Duration,
    tracing::instrument,
    url::Url,
};

#[instrument(skip_all)]
pub async fn run(notify_url: &Url, keypair: &Keypair, client: &Client) -> Result<(), Error> {
    client
        .watch_register(
            WatchRegisterRequest {
                service_url: notify_url.to_string(),
                webhook_url: notify_url
                    .join(RELAY_WEBHOOK_ENDPOINT)
                    .expect("Should be able to join static URLs")
                    .to_string(),
                watch_type: WatchType::Subscriber,
                tags: vec![
                    NOTIFY_SUBSCRIBE_TAG,
                    NOTIFY_DELETE_TAG,
                    NOTIFY_UPDATE_TAG,
                    NOTIFY_WATCH_SUBSCRIPTIONS_TAG,
                    NOTIFY_GET_NOTIFICATIONS_TAG,
                ],
                statuses: vec![WatchStatus::Queued],
                ttl: Duration::from_secs(60 * 60 * 24 * 30),
            },
            keypair,
        )
        .await?;
    Ok(())
}
