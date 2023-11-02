use {
    relay_client::{error::Error, http::Client},
    relay_rpc::rpc::{msg_id::MsgId, Publish},
    std::time::{Duration, Instant},
    tokio::time::sleep,
    tracing::{instrument, warn},
};

#[instrument(skip_all)]
pub async fn publish_relay_message(
    relay_http_client: &Client,
    publish: &Publish,
) -> Result<(), Error> {
    let start = Instant::now();

    let message_id = publish.msg_id();

    let client_publish_call = || {
        relay_http_client.publish(
            publish.topic.clone(),
            publish.message.clone(),
            publish.tag,
            Duration::from_secs(publish.ttl_secs as u64) - start.elapsed(),
            publish.prompt,
        )
    };

    let mut tries = 0;
    while let Err(e) = client_publish_call().await {
        tries += 1;
        if tries >= 10 {
            return Err(e);
        }

        let retry_in = Duration::from_secs(1);
        warn!(
            "Temporary error on publishing message {message_id} to topic {}, \
            retrying attempt {tries} in {retry_in:?}: {e:?}",
            publish.topic,
        );
        sleep(retry_in).await;
    }

    Ok(())
}
