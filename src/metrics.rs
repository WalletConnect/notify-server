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
    pub dispatched_notifications: Counter<u64>,
    pub notify_latency: Histogram<u64>,
    http_requests: Counter<u64>,
    http_request_latency: Histogram<u64>,
    pub processed_notifications: Counter<u64>,
    relay_incomming_messages: Counter<u64>,
    relay_incomming_message_latency: Histogram<u64>,
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

        let dispatched_notifications = meter
            .u64_counter("dispatched_notifications")
            .with_description("The number of notification dispatched in one request")
            .init();

        let notify_latency = meter
            .u64_histogram("notify_latency")
            .with_description("The amount of time it took to dispatch all notifications")
            .init();

        let http_requests = meter
            .u64_counter("http_requests")
            .with_description("The number of HTTP requests handled")
            .init();

        let http_request_latency: Histogram<u64> = meter
            .u64_histogram("http_request_latency")
            .with_description("The latency handling HTTP requests")
            .init();

        let processed_notifications = meter
            .u64_counter("processed_notifications")
            .with_description("The number of processed notifications")
            .init();

        let relay_incomming_messages = meter
            .u64_counter("relay_incomming_messages")
            .with_description("The number of relay messages handled")
            .init();

        let relay_incomming_message_latency: Histogram<u64> = meter
            .u64_histogram("relay_message_latency")
            .with_description("The latency handling relay messages")
            .init();

        Metrics {
            subscribed_project_topics,
            subscribed_subscriber_topics,
            subscribe_latency,
            dispatched_notifications,
            notify_latency,
            http_requests,
            http_request_latency,
            processed_notifications,
            relay_incomming_messages,
            relay_incomming_message_latency,
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

    pub fn relay_incomming_message(
        &self,
        tag: u32,
        status: RelayIncommingMessageStatus,
        start: std::time::Instant,
    ) {
        let elapsed = start.elapsed();

        let ctx = Context::current();
        let attributes = [
            KeyValue::new("tag", tag.to_string()),
            KeyValue::new("status", status.to_string()),
        ];
        self.relay_incomming_messages.add(&ctx, 1, &attributes);
        self.relay_incomming_message_latency
            .record(&ctx, elapsed.as_millis() as u64, &attributes);
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

pub enum RelayIncommingMessageStatus {
    Success,
    ServerError,
}

impl Display for RelayIncommingMessageStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Success => write!(f, "success"),
            Self::ServerError => write!(f, "server_error"),
        }
    }
}
