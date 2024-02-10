use {
    relay_client::http::Client,
    relay_rpc::rpc::Receipt,
    std::{convert::Infallible, sync::Arc, time::Duration},
    tokio::{
        select,
        sync::mpsc::{Receiver, Sender},
    },
    tracing::{info, warn},
};

const BATCH_SIZE: usize = 500;
pub const BATCH_TIMEOUT: Duration = Duration::from_secs(5);

pub async fn start(
    relay_client: Arc<Client>,
    batch_receive_rx: Receiver<Receipt>,
) -> Result<(), Infallible> {
    let (output_tx, mut output_rx) = tokio::sync::mpsc::channel(1);
    let relay_handler = async {
        while let Some(receipts) = output_rx.recv().await {
            if let Err(e) = relay_client.batch_receive(receipts).await {
                // Failure is not a major issue, as messages will expire from the mailbox after their TTL anyway
                warn!("Error while calling batch_receive: {e:?}");
                // TODO retry
                // TODO metrics
            }
        }
        info!("output_rx closed");
        Ok(())
    };
    select! {
        e = batcher::<BATCH_SIZE, _>(BATCH_TIMEOUT, batch_receive_rx, output_tx) => e,
        e = relay_handler => e,
    }
}

/// Accepts items from `items_to_batch`, batches them up to `MAX_BATCH_SIZE`, and sends batches to `output_batches`.
/// If an item is not received for `timeout` and a partial batch ready, then the partial batch will be sent.
/// Note: MATCH_BATCH_SIZE must be at least 2 or the behavior is undefined.
async fn batcher<const MAX_BATCH_SIZE: usize, T>(
    timeout: Duration,
    mut items_to_batch: Receiver<T>,
    output_batches: Sender<Vec<T>>,
) -> Result<(), Infallible> {
    let mut batch_buffer = Vec::with_capacity(MAX_BATCH_SIZE);

    async fn send_batch<const MAX_BATCH_SIZE: usize, T>(
        batch: Vec<T>,
        output_batches: &Sender<Vec<T>>,
    ) -> Option<Vec<T>> {
        assert!(!batch.is_empty());
        assert!(batch.len() <= MAX_BATCH_SIZE);
        if let Err(e) = output_batches.send(batch).await {
            info!("output_batches closed: {e:?}");
            return None;
        }
        Some(Vec::with_capacity(MAX_BATCH_SIZE))
    }

    loop {
        if batch_buffer.is_empty() {
            match items_to_batch.recv().await {
                Some(item) => {
                    batch_buffer.push(item);
                }
                None => {
                    info!("items_to_batch closed");
                    break;
                }
            }
        } else {
            match tokio::time::timeout(timeout, items_to_batch.recv()).await {
                Ok(Some(item)) => {
                    batch_buffer.push(item);
                    if batch_buffer.len() >= MAX_BATCH_SIZE {
                        batch_buffer = if let Some(batch_buffer) =
                            send_batch::<MAX_BATCH_SIZE, _>(batch_buffer, &output_batches).await
                        {
                            batch_buffer
                        } else {
                            break;
                        };
                    }
                }
                Ok(None) => {
                    info!("items_to_batch closed");
                    break;
                }
                Err(_) => {
                    batch_buffer = if let Some(batch_buffer) =
                        send_batch::<MAX_BATCH_SIZE, _>(batch_buffer, &output_batches).await
                    {
                        batch_buffer
                    } else {
                        break;
                    };
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (Sender<usize>, Receiver<Vec<usize>>) {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        let (output_tx, output_rx) = tokio::sync::mpsc::channel(1);
        tokio::task::spawn(batcher::<10, _>(Duration::from_millis(100), rx, output_tx));
        (tx, output_rx)
    }

    #[tokio::test]
    async fn sends_one_after_timeout() {
        let (tx, mut output_rx) = setup();
        tx.send(1).await.unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_millis(150), output_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            vec![1]
        );
    }

    #[tokio::test]
    async fn sends_batch_instantly() {
        let (tx, mut output_rx) = setup();
        for i in 0..10 {
            tx.send(i).await.unwrap();
        }
        assert_eq!(
            tokio::time::timeout(Duration::from_millis(10), output_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            (0..10).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn sends_two_batches_instantly() {
        let (tx, mut output_rx) = setup();
        for i in 0..20 {
            tx.send(i).await.unwrap();
        }
        assert_eq!(
            tokio::time::timeout(Duration::from_millis(10), output_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            (0..10).collect::<Vec<_>>()
        );
        assert_eq!(
            tokio::time::timeout(Duration::from_millis(10), output_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            (10..20).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn sends_two_batches_second_after_timeout() {
        let (tx, mut output_rx) = setup();
        for i in 0..19 {
            tx.send(i).await.unwrap();
        }
        assert_eq!(
            tokio::time::timeout(Duration::from_millis(10), output_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            (0..10).collect::<Vec<_>>()
        );
        assert_eq!(
            tokio::time::timeout(Duration::from_millis(110), output_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            (10..19).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn timeout_reset_after_each_item() {
        let (tx, mut output_rx) = setup();
        for i in 0..10 {
            tx.send(i).await.unwrap();
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(
            tokio::time::timeout(Duration::from_millis(110), output_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            (0..10).collect::<Vec<_>>(),
        );
    }

    #[tokio::test]
    async fn does_not_send_empty_batches() {
        let (tx, mut output_rx) = setup();

        // Nothing received initially
        assert!(
            tokio::time::timeout(Duration::from_millis(150), output_rx.recv())
                .await
                .is_err()
        );

        // Get a batch
        tx.send(1).await.unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_millis(150), output_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            vec![1]
        );

        // Nothing received after
        assert!(
            tokio::time::timeout(Duration::from_millis(150), output_rx.recv())
                .await
                .is_err()
        );
    }
}
