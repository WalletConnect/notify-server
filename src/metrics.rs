use wc::metrics::otel::metrics::{Counter, Histogram, UpDownCounter};

#[derive(Clone)]
pub struct Metrics {
    pub registered_clients: UpDownCounter<i64>,
    pub dispatched_notifications: Counter<u64>,
    pub send_latency: Histogram<u64>,
}

impl Metrics {
    pub fn new() -> Self {
        let meter = wc::metrics::ServiceMetrics::meter();

        let clients_counter = meter
            .i64_up_down_counter("registered_clients")
            .with_description("The number of currently registered clients")
            .init();

        let dispatched_notifications = meter
            .u64_counter("dispatched_notifications")
            .with_description("The number of notification dispatched in one request")
            .init();

        let send_latency = meter
            .u64_histogram("notify_latency")
            .with_description("The amount of time it took to dispatch all notifications")
            .init();

        Metrics {
            registered_clients: clients_counter,
            dispatched_notifications,
            send_latency,
        }
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}
