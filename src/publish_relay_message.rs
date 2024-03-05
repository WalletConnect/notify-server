use {
    crate::metrics::Metrics,
    relay_client::{error::Error, http::Client},
    relay_rpc::{
        domain::Topic,
        rpc::{self, msg_id::get_message_id, Publish, PublishError, SubscriptionError},
    },
    std::time::{Duration, Instant},
    tokio::time::sleep,
    tracing::{error, info, instrument, warn},
};

/// Calculate the time before retrying again. Input how many times the action has been attempted so far (i.e. should start at 1).
/// First retry will be instant. The 10th retry will be approx 4s.
fn calculate_retry_in(tries: i32) -> Duration {
    let tries = tries as f32;
    let secs = 0.05 * (tries - 1.).powf(2.);
    Duration::from_millis((secs * 1000.) as u64)
}

#[instrument(skip_all, fields(topic = %publish.topic, tag = %publish.tag, message_id = %get_message_id(&publish.message)))]
pub async fn publish_relay_message(
    relay_client: &Client,
    publish: &Publish,
    metrics: Option<&Metrics>,
) -> Result<(), Error<PublishError>> {
    info!("publish_relay_message");
    let start = Instant::now();

    let call = || async {
        let start = Instant::now();
        let result = relay_client
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
        match result {
            Ok(_) => Ok(()),
            Err(e) => match e {
                Error::Response(rpc::Error::Handler(PublishError::MailboxLimitExceeded)) => {
                    // Only happens if there is no subscriber for the topic in the first place.
                    // So client is not expecting a response, or in the case of notify messages
                    // should use getNotifications to get old notifications anyway.
                    info!("Mailbox limit exceeded for topic {}", publish.topic);
                    Ok(())
                }
                e => Err(e),
            },
        }
    };

    let mut tries = 0;
    while let Err(e) = call().await {
        tries += 1;
        let is_permanent = tries >= 10;
        if let Some(metrics) = metrics {
            metrics.relay_outgoing_message_failure(publish.tag, is_permanent);
        }

        if is_permanent {
            error!("Permanent error publishing message, took {tries} tries: {e:?}");

            if let Some(metrics) = metrics {
                // TODO make DRY with end-of-function call
                metrics.relay_outgoing_message(publish.tag, false, start);
            }
            return Err(e);
        }

        let retry_in = calculate_retry_in(tries);
        warn!(
            "Temporary error publishing message, retrying attempt {tries} in {retry_in:?}: {e:?}",
        );
        sleep(retry_in).await;
    }

    info!("Sucessfully published message");

    if let Some(metrics) = metrics {
        metrics.relay_outgoing_message(publish.tag, true, start);
    }
    Ok(())
}

#[instrument(skip(relay_client, metrics))]
pub async fn subscribe_relay_topic(
    relay_client: &Client,
    topic: &Topic,
    metrics: Option<&Metrics>,
) -> Result<(), Error<SubscriptionError>> {
    info!("subscribe_relay_topic");
    let start = Instant::now();

    let call = || async {
        let start = Instant::now();
        let result = relay_client.subscribe_blocking(topic.clone()).await;
        if let Some(metrics) = metrics {
            metrics.relay_subscribe_request(start);
        }
        result
    };

    let mut tries = 0;
    while let Err(e) = call().await {
        tries += 1;
        let is_permanent = tries >= 10;
        if let Some(metrics) = metrics {
            metrics.relay_subscribe_failure(is_permanent);
        }

        if is_permanent {
            error!("Permanent error subscribing to topic, took {tries} tries: {e:?}");

            if let Some(metrics) = metrics {
                // TODO make DRY with end-of-function call
                metrics.relay_subscribe(false, start);
            }
            return Err(e);
        }

        let retry_in = calculate_retry_in(tries);
        warn!(
            "Temporary error subscribing to topic, retrying attempt {tries} in {retry_in:?}: {e:?}"
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

#[instrument(skip(relay_client, metrics))]
pub async fn batch_subscribe_relay_topics(
    relay_client: &Client,
    topics: Vec<Topic>,
    metrics: Option<&Metrics>,
) -> Result<(), Error<SubscriptionError>> {
    info!("batch_subscribe_relay_topic");
    let start = Instant::now();

    let call = || async {
        let start = Instant::now();
        let result = relay_client.batch_subscribe_blocking(topics.clone()).await;
        if let Some(metrics) = metrics {
            metrics.relay_batch_subscribe_request(start);
        }
        // TODO process each error individually
        // TODO retry relay internal failures?
        // https://github.com/WalletConnect/notify-server/issues/395
        result
    };

    let mut tries = 0;
    while let Err(e) = call().await {
        tries += 1;
        let is_permanent = tries >= 10;
        if let Some(metrics) = metrics {
            metrics.relay_batch_subscribe_failure(is_permanent);
        }

        if is_permanent {
            error!("Permanent error batch subscribing to topics, took {tries} tries: {e:?}");

            if let Some(metrics) = metrics {
                // TODO make DRY with end-of-function call
                metrics.relay_batch_subscribe(false, start);
            }
            return Err(e);
        }

        let retry_in = calculate_retry_in(tries);
        warn!(
            "Temporary error batch subscribing to topics, retrying attempt {tries} in {retry_in:?}: {e:?}"
        );
        sleep(retry_in).await;
    }

    if let Some(metrics) = metrics {
        metrics.relay_batch_subscribe(true, start);
    }

    // Sleep to account for some replication lag. Without this, the subscription may not be active on all nodes
    sleep(Duration::from_millis(250)).await;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limit_retries_instantly_first_try() {
        assert_eq!(calculate_retry_in(1), Duration::ZERO);
    }

    #[test]
    fn rate_limit_retries_after_delay_second_try() {
        assert!(calculate_retry_in(2) > Duration::ZERO);
    }

    #[test]
    fn rate_limit_retries_after_10_tries_within_10s() {
        assert!(calculate_retry_in(10) < Duration::from_secs(10));
    }
}
