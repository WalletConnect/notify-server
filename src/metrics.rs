use wc::metrics::otel::metrics::{Counter, Histogram, ObservableGauge};

#[derive(Clone)]
pub struct Metrics {
    pub subscribed_project_topics: ObservableGauge<u64>,
    pub subscribed_client_topics: ObservableGauge<u64>,
    pub subscribe_latency: Histogram<u64>,
    pub dispatched_notifications: Counter<u64>,
    pub notify_latency: Histogram<u64>,
}

impl Metrics {
    pub fn new() -> Self {
        let meter = wc::metrics::ServiceMetrics::meter();

        let subscribed_project_topics = meter
            .u64_observable_gauge("subscribed_project_topics")
            .with_description("The number of subscribed project topics")
            .init();

        let subscribed_client_topics = meter
            .u64_observable_gauge("subscribed_client_topics")
            .with_description("The number of subscribed client topics")
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

        Metrics {
            subscribed_project_topics,
            subscribed_client_topics,
            subscribe_latency,
            dispatched_notifications,
            notify_latency,
        }
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}
