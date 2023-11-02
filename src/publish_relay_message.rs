use {
    relay_client::{error::Error, http::Client},
    relay_rpc::rpc::{msg_id::MsgId, Publish},
    std::time::{Duration, Instant},
    tokio::time::sleep,
    tracing::{error, instrument, warn},
};

#[instrument(skip_all)]
pub async fn publish_relay_message(
    relay_http_client: &Client,
    publish: &Publish,
) -> Result<(), Error> {
    let start = Instant::now();

    let client_publish_call = || {
        relay_http_client.publish(
            publish.topic.clone(),
            publish.message.clone(),
            publish.tag,
            Duration::from_secs(publish.ttl_secs as u64) - start.elapsed(),
            publish.prompt,
        )
    };

    // Avoid hashing on the first iteration since we only need it for the logged failure cases
    let mut cached_message_id = None;

    let mut tries = 0;
    while let Err(e) = client_publish_call().await {
        // Since message ID is a hash, avoid rehashing it on each iteration
        let message_id = match cached_message_id.as_ref() {
            Some(message_id) => message_id,
            None => {
                cached_message_id = Some(publish.msg_id());
                cached_message_id.as_ref().unwrap()
            }
        };

        tries += 1;
        if tries >= 10 {
            error!(
                "Permenant error publishing message {message_id} to topic {}, took {tries} tries: {e:?}",
                publish.topic,
            );
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
