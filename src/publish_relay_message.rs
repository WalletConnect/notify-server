use {
    crate::metrics::Metrics,
    relay_client::{error::Error, http::Client},
    relay_rpc::{
        domain::Topic,
        rpc::{msg_id::MsgId, Publish},
    },
    std::time::{Duration, Instant},
    tokio::time::sleep,
    tracing::{error, instrument, warn},
};

#[instrument(skip_all)]
pub async fn publish_relay_message(
    relay_http_client: &Client,
    publish: &Publish,
    metrics: Option<&Metrics>,
) -> Result<(), Error> {
    let start = Instant::now();

    let client_publish_call = || async {
        let start = Instant::now();
        let result = relay_http_client
            .publish(
                publish.topic.clone(),
                publish.message.clone(),
                publish.tag,
                Duration::from_secs(publish.ttl_secs as u64) - start.elapsed(),
                publish.prompt,
            )
            .await;
        if let Some(metrics) = metrics {
            metrics.relay_outgoing_message_publish(publish.tag, start);
        }
        result
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
        let is_permenant = tries >= 10;
        if let Some(metrics) = metrics {
            metrics.relay_outgoing_message_failure(publish.tag, is_permenant);
        }

        if is_permenant {
            error!(
                "Permenant error publishing message {message_id} to topic {}, took {tries} tries: {e:?}",
                publish.topic,
            );

            if let Some(metrics) = metrics {
                // TODO make DRY with end-of-function call
                metrics.relay_outgoing_message(publish.tag, false, start);
            }
            return Err(e);
        }

        let retry_in = Duration::from_secs(1);
        warn!(
            "Temporary error publishing message {message_id} to topic {}, \
            retrying attempt {tries} in {retry_in:?}: {e:?}",
            publish.topic,
        );
        sleep(retry_in).await;
    }

    if let Some(metrics) = metrics {
        metrics.relay_outgoing_message(publish.tag, true, start);
    }
    Ok(())
}

#[instrument(skip_all)]
pub async fn subscribe_relay_topic(
    relay_ws_client: &relay_client::websocket::Client,
    topic: &Topic,
    metrics: Option<&Metrics>,
) -> Result<(), Error> {
    let start = Instant::now();

    let client_publish_call = || async {
        let start = Instant::now();
        let result = relay_ws_client.subscribe_blocking(topic.clone()).await;
        if let Some(metrics) = metrics {
            metrics.relay_subscribe_request(start);
        }
        result
    };

    let mut tries = 0;
    while let Err(e) = client_publish_call().await {
        tries += 1;
        let is_permenant = tries >= 10;
        if let Some(metrics) = metrics {
            metrics.relay_subscribe_failure(is_permenant);
        }

        if is_permenant {
            error!("Permenant error subscribing to topic {topic}, took {tries} tries: {e:?}");

            if let Some(metrics) = metrics {
                // TODO make DRY with end-of-function call
                metrics.relay_subscribe(false, start);
            }
            return Err(e);
        }

        let retry_in = Duration::from_secs(1);
        warn!(
            "Temporary error subscribing to topic {topic}, retrying attempt {tries} in {retry_in:?}: {e:?}"
        );
        sleep(retry_in).await;
    }

    if let Some(metrics) = metrics {
        metrics.relay_subscribe(true, start);
    }

    // Sleep to account for some replication lag. Without this, the subscription may not be active on all nodes
    sleep(Duration::from_millis(250)).await;

    Ok(())
}
