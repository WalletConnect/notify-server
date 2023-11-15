use {
    relay_client::websocket::ConnectionHandler,
    tokio::sync::mpsc,
    tracing::{error, info},
    tungstenite::protocol::CloseFrame,
};

pub struct RelayConnectionHandler {
    name: &'static str,
    tx: mpsc::UnboundedSender<RelayClientEvent>,
}

#[derive(Debug)]
pub enum RelayClientEvent {
    Message(relay_client::websocket::PublishedMessage),
    Error(relay_client::error::Error),
    Disconnected(Option<CloseFrame<'static>>),
    Connected,
}

impl RelayConnectionHandler {
    pub fn new(name: &'static str, tx: mpsc::UnboundedSender<RelayClientEvent>) -> Self {
        Self { name, tx }
    }
}

impl ConnectionHandler for RelayConnectionHandler {
    fn connected(&mut self) {
        info!("[{}] connection open", self.name);
        if let Err(e) = self.tx.send(RelayClientEvent::Connected) {
            error!("[{}] failed to emit the connection event: {}", self.name, e);
        }
    }

    fn disconnected(&mut self, frame: Option<CloseFrame<'static>>) {
        info!("[{}] connection closed: frame={frame:?}", self.name);
        if let Err(e) = self.tx.send(RelayClientEvent::Disconnected(frame)) {
            error!(
                "[{}] failed to emit the disconnection event: {}",
                self.name, e
            );
        }
    }

    fn message_received(&mut self, message: relay_client::websocket::PublishedMessage) {
        info!(
            "[{}] inbound message: topic={} message={}",
            self.name, message.topic, message.message
        );
        if let Err(e) = self.tx.send(RelayClientEvent::Message(message)) {
            error!("[{}] failed to emit the message event: {}", self.name, e);
        }
    }

    fn inbound_error(&mut self, error: relay_client::error::Error) {
        info!("[{}] inbound error: {error}", self.name);
        if let Err(e) = self.tx.send(RelayClientEvent::Error(error)) {
            error!(
                "[{}] failed to emit the inbound error event: {}",
                self.name, e
            );
        }
    }

    fn outbound_error(&mut self, error: relay_client::error::Error) {
        info!("[{}] outbound error: {error}", self.name);
        if let Err(e) = self.tx.send(RelayClientEvent::Error(error)) {
            error!(
                "[{}] failed to emit the outbound error event: {}",
                self.name, e
            );
        }
    }
}
