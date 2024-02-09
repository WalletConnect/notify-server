use {
    relay_client::http::Client,
    relay_rpc::rpc::Receipt,
    std::{convert::Infallible, sync::Arc, time::Duration},
    tokio::{
        select,
        sync::mpsc::{Receiver, Sender},
    },
    tracing::warn,
};

pub async fn start(
    relay_client: Arc<Client>,
    batch_receive_rx: Receiver<Receipt>,
) -> Result<(), Infallible> {
    let (output_tx, mut output_rx) = tokio::sync::mpsc::channel(1);
    let relay_handler = async {
        while let Some(receipts) = output_rx.recv().await {
            if let Err(e) = relay_client.batch_receive(receipts).await {
                warn!("Error while batch receiving: {e:?}");
            }
        }
        warn!("output_rx closed");
        Ok(())
    };
    select! {
        e = batcher::<500, _>(Duration::from_secs(5), output_tx, batch_receive_rx) => e,
        e = relay_handler => e,
    }
}

/// Accepts items from `items_to_batch`, batches them up to `MAX_BATCH_SIZE`, and sends batches to `output_batches`.
/// If an item is not received for `timeout` and a partial batch ready, then the partial batch will be sent.
/// Note: MATCH_BATCH_SIZE must be at least 2 or the behavior is undefined.
async fn batcher<const MAX_BATCH_SIZE: usize, T>(
    timeout: Duration,
    output_batches: Sender<Vec<T>>,
    mut items_to_batch: Receiver<T>,
) -> Result<(), Infallible> {
    let mut batch_buffer = Vec::with_capacity(MAX_BATCH_SIZE);

    async fn send<const MAX_BATCH_SIZE: usize, T>(
        batch: Vec<T>,
        batch_sender_tx: &Sender<Vec<T>>,
    ) -> Vec<T> {
        assert!(!batch.is_empty());
        assert!(batch.len() <= MAX_BATCH_SIZE);
        if let Err(e) = batch_sender_tx.send(batch).await {
            warn!("Error while sending to batch_sender_tx: {e:?}");
        }
        Vec::with_capacity(MAX_BATCH_SIZE)
    }

    loop {
        if batch_buffer.is_empty() {
            match items_to_batch.recv().await {
                Some(item) => {
                    batch_buffer.push(item);
                }
                None => break,
            }
        } else {
            match tokio::time::timeout(timeout, items_to_batch.recv()).await {
                Ok(Some(item)) => {
                    batch_buffer.push(item);
                    if batch_buffer.len() >= MAX_BATCH_SIZE {
                        batch_buffer =
                            send::<MAX_BATCH_SIZE, _>(batch_buffer, &output_batches).await;
                    }
                }
                Ok(None) => break,
                Err(_) => {
                    batch_buffer = send::<MAX_BATCH_SIZE, _>(batch_buffer, &output_batches).await;
                }
            }
        }
    }

    warn!("batch_receive_rx closed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (Sender<usize>, Receiver<Vec<usize>>) {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        let (output_tx, output_rx) = tokio::sync::mpsc::channel(1);
        tokio::task::spawn(batcher::<10, _>(Duration::from_millis(100), output_tx, rx));
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
