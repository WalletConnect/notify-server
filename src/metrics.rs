use {
    crate::state::AppState,
    axum::{
        extract::{MatchedPath, State},
        http::Method,
        middleware::Next,
        response::Response,
    },
    core::fmt,
    hyper::{Request, StatusCode},
    std::{
        fmt::{Display, Formatter},
        sync::Arc,
        time::Instant,
    },
    wc::metrics::otel::{
        metrics::{Counter, Histogram, ObservableGauge},
        Context, KeyValue,
    },
};

#[derive(Clone)]
pub struct Metrics {
    pub subscribed_project_topics: ObservableGauge<u64>,
    pub subscribed_subscriber_topics: ObservableGauge<u64>,
    pub subscribe_latency: Histogram<u64>,
    http_requests: Counter<u64>,
    http_request_latency: Histogram<u64>,
    relay_incoming_messages: Counter<u64>,
    relay_incoming_message_latency: Histogram<u64>,
    relay_outgoing_messages: Counter<u64>,
    relay_outgoing_message_failures: Counter<u64>,
    relay_outgoing_message_latency: Histogram<u64>,
    relay_outgoing_message_publish_latency: Histogram<u64>,
    pub processed_notifications: Counter<u64>,
    pub dispatched_notifications: Counter<u64>,
    pub notify_latency: Histogram<u64>,
    pub publishing_workers_count: ObservableGauge<u64>,
    pub publishing_workers_errors: Counter<u64>,
    pub publishing_queue_queued_size: ObservableGauge<u64>,
    pub publishing_queue_processing_size: ObservableGauge<u64>,
    pub publishing_queue_published_count: Counter<u64>,
}

impl Metrics {
    pub fn new() -> Self {
        let meter = wc::metrics::ServiceMetrics::meter();

        let subscribed_project_topics = meter
            .u64_observable_gauge("subscribed_project_topics")
            .with_description("The number of subscribed project topics")
            .init();

        let subscribed_subscriber_topics = meter
            .u64_observable_gauge("subscribed_subscriber_topics")
            .with_description("The number of subscribed subscriber topics")
            .init();

        let subscribe_latency = meter
            .u64_histogram("subscribe_latency")
            .with_description("The amount of time it took to subscribe to all topics")
            .init();

        let http_requests = meter
            .u64_counter("http_requests")
            .with_description("The number of HTTP requests handled")
            .init();

        let http_request_latency: Histogram<u64> = meter
            .u64_histogram("http_request_latency")
            .with_description("The latency handling HTTP requests")
            .init();

        let relay_incoming_messages = meter
            .u64_counter("relay_incoming_messages")
            .with_description("The number of relay messages handled")
            .init();

        let relay_incoming_message_latency: Histogram<u64> = meter
            .u64_histogram("relay_incoming_message_latency")
            .with_description("The latency handling relay messages")
            .init();

        let relay_outgoing_messages: Counter<u64> = meter
            .u64_counter("relay_outgoing_messages")
            .with_description(
                "The number of outgoing relay messages being published (not including retries)",
            )
            .init();

        let relay_outgoing_message_failures: Counter<u64> = meter
            .u64_counter("relay_outgoing_message_failures")
            .with_description("The number of publish fails")
            .init();

        let relay_outgoing_message_latency: Histogram<u64> = meter
            .u64_histogram("relay_outgoing_message_latency")
            .with_description("The latency publishing relay messages w/ built-in retry")
            .init();

        let relay_outgoing_message_publish_latency: Histogram<u64> = meter
            .u64_histogram("relay_outgoing_message_publish_latency")
            .with_description("The latency publishing relay messages")
            .init();

        let processed_notifications = meter
            .u64_counter("processed_notifications")
            .with_description("The number of processed notifications")
            .init();

        let dispatched_notifications = meter
            .u64_counter("dispatched_notifications")
            .with_description("The number of notification dispatched in one request")
            .init();

        let notify_latency = meter
            .u64_histogram("notify_latency")
            .with_description("The amount of time it took to dispatch all notifications")
            .init();

        let publishing_workers_count = meter
            .u64_observable_gauge("publishing_workers_count")
            .with_description("The number of spawned publishing workers tasks")
            .init();

        let publishing_workers_errors = meter
            .u64_counter("publishing_workers_errors")
            .with_description("The number of publishing worker that ended with an error")
            .init();

        let publishing_queue_queued_size = meter
            .u64_observable_gauge("publishing_queue_queued_size")
            .with_description("The messages publishing queue size in queued state")
            .init();

        let publishing_queue_processing_size = meter
            .u64_observable_gauge("publishing_queue_processing_size")
            .with_description("The messages publishing queue size in processing state")
            .init();

        let publishing_queue_published_count = meter
            .u64_counter("publishing_queue_published_count")
            .with_description("The number of published messages by workers")
            .init();

        Metrics {
            subscribed_project_topics,
            subscribed_subscriber_topics,
            subscribe_latency,
            http_requests,
            http_request_latency,
            relay_incoming_messages,
            relay_incoming_message_latency,
            relay_outgoing_messages,
            relay_outgoing_message_failures,
            relay_outgoing_message_latency,
            relay_outgoing_message_publish_latency,
            processed_notifications,
            dispatched_notifications,
            notify_latency,
            publishing_workers_count,
            publishing_workers_errors,
            publishing_queue_queued_size,
            publishing_queue_processing_size,
            publishing_queue_published_count,
        }
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

impl Metrics {
    pub fn http_request(
        &self,
        endpoint: &str,
        method: &str,
        status: StatusCode,
        start: std::time::Instant,
    ) {
        let elapsed = start.elapsed();

        let ctx = Context::current();
        let attributes = [
            KeyValue::new("endpoint", endpoint.to_owned()),
            KeyValue::new("method", method.to_owned()),
            KeyValue::new("status", status.as_u16().to_string()),
        ];
        self.http_requests.add(&ctx, 1, &attributes);
        self.http_request_latency
            .record(&ctx, elapsed.as_millis() as u64, &attributes);
    }

    pub fn relay_incoming_message(
        &self,
        tag: u32,
        status: RelayIncomingMessageStatus,
        start: std::time::Instant,
    ) {
        let elapsed = start.elapsed();

        let ctx = Context::current();
        let attributes = [
            KeyValue::new("tag", tag.to_string()),
            KeyValue::new("status", status.to_string()),
        ];
        self.relay_incoming_messages.add(&ctx, 1, &attributes);
        self.relay_incoming_message_latency
            .record(&ctx, elapsed.as_millis() as u64, &attributes);
    }

    pub fn relay_outgoing_message(&self, tag: u32, success: bool, start: std::time::Instant) {
        let elapsed = start.elapsed();

        let ctx = Context::current();
        let attributes = [
            KeyValue::new("tag", tag.to_string()),
            KeyValue::new("success", success.to_string()),
        ];
        self.relay_outgoing_messages.add(&ctx, 1, &attributes);
        self.relay_outgoing_message_latency
            .record(&ctx, elapsed.as_millis() as u64, &attributes);
    }

    pub fn relay_outgoing_message_failure(&self, tag: u32, is_permenant: bool) {
        let ctx = Context::current();
        let attributes = [
            KeyValue::new("tag", tag.to_string()),
            KeyValue::new("is_permenant", is_permenant.to_string()),
        ];
        self.relay_outgoing_message_failures
            .add(&ctx, 1, &attributes);
    }

    pub fn relay_outgoing_message_publish(&self, tag: u32, start: std::time::Instant) {
        let elapsed = start.elapsed();

        let ctx = Context::current();
        let attributes = [KeyValue::new("tag", tag.to_string())];
        self.relay_outgoing_message_publish_latency.record(
            &ctx,
            elapsed.as_millis() as u64,
            &attributes,
        );
    }
}

pub async fn http_request_middleware<B>(
    State(state): State<Arc<AppState>>,
    path: MatchedPath,
    method: Method,
    request: Request<B>,
    next: Next<B>,
) -> Response {
    let start = Instant::now();
    let response = next.run(request).await;
    if let Some(metrics) = &state.metrics {
        metrics.http_request(path.as_str(), method.as_str(), response.status(), start);
    }
    response
}

pub enum RelayIncomingMessageStatus {
    Success,
    ServerError,
}

impl Display for RelayIncomingMessageStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Success => write!(f, "success"),
            Self::ServerError => write!(f, "server_error"),
        }
    }
}
