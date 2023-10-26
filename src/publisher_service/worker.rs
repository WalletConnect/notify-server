use {
    sqlx::{postgres::PgListener, PgPool},
    std::sync::Arc,
};

pub async fn run(
    pg_pool: &Arc<PgPool>,
    relay_client: &Arc<relay_client::http::Client>,
) -> Result<(), sqlx::Error> {
    let mut pg_notify_listener = PgListener::connect_with(pg_pool)
        .await
        .expect("Notifying listener failed to connect to Postgres");
    pg_notify_listener
        .listen("notification_for_delivery")
        .await
        .expect("Failed to listen to Postgres events channels");

    loop {
        let notification = pg_notify_listener.recv().await?;
        tokio::spawn({
            let pg_pool = pg_pool.clone();
            let relay_client = relay_client.clone();
            async move { process_message(&pg_pool, &relay_client, notification.payload()).await }
        });
    }
}

async fn process_message(
    _pg_pool: &Arc<PgPool>,
    _relay_client: &Arc<relay_client::http::Client>,
    _message_delivery_id: &str,
) {
    // TODO: implement
}
