use {
    crate::state::AppState,
    axum::{
        extract::{MatchedPath, State},
        http::Method,
        middleware::Next,
        response::Response,
    },
    hyper::Request,
    std::{sync::Arc, time::Instant},
    wc::metrics::otel::{
        metrics::{Counter, Histogram, ObservableGauge},
        Context, KeyValue,
    },
};

#[derive(Clone)]
pub struct Metrics {
    pub subscribed_topics: ObservableGauge<u64>,
    pub subscribe_latency: Histogram<u64>,
    pub dispatched_notifications: Counter<u64>,
    pub notify_latency: Histogram<u64>,
    http_requests: Counter<u64>,
    http_request_latency: Histogram<u64>,
    // relay_messages: Counter<u64>,
    // relay_message_latency: Histogram<u64>,
}

impl Metrics {
    pub fn new() -> Self {
        let meter = wc::metrics::ServiceMetrics::meter();

        let subscribed_topics = meter
            .u64_observable_gauge("subscribed_topics")
            .with_description("The number of subscribed topics")
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

        // let relay_messages = meter
        //     .u64_counter("relay_messages")
        //     .with_description("The number of relay messages handled")
        //     .init();

        // let relay_message_latency: Histogram<u64> = meter
        //     .u64_histogram("relay_message_latency")
        //     .with_description("The latency handling relay messages")
        //     .init();

        Metrics {
            subscribed_topics,
            subscribe_latency,
            dispatched_notifications,
            notify_latency,
            http_requests,
            http_request_latency,
            // relay_messages,
            // relay_message_latency,
        }
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

impl Metrics {
    pub fn http_request(&self, endpoint: &str, method: &str, start: std::time::Instant) {
        let elapsed = start.elapsed();

        let ctx = Context::current();
        let attributes = [
            KeyValue::new("endpoint", endpoint.to_owned()),
            KeyValue::new("method", method.to_owned()),
        ];
        self.http_requests.add(&ctx, 1, &attributes);
        self.http_request_latency
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
        metrics.http_request(path.as_str(), method.as_str(), start);
    }
    response
}
