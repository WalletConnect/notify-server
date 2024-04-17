use {
    crate::{auth::KeysServerResponseSource, state::AppState},
    axum::{
        extract::{MatchedPath, Request, State},
        http::Method,
        middleware::Next,
        response::Response,
    },
    core::fmt,
    hyper::StatusCode,
    std::{
        fmt::{Display, Formatter},
        sync::Arc,
        time::Instant,
    },
    wc::metrics::{
        otel::{
            self,
            metrics::{Counter, Histogram, ObservableGauge},
            KeyValue,
        },
        otel_sdk::metrics::{Aggregation, Instrument, Stream},
        ServiceMetrics,
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
    relay_subscribes: Counter<u64>,
    relay_subscribe_failures: Counter<u64>,
    relay_subscribe_latency: Histogram<u64>,
    relay_subscribe_request_latency: Histogram<u64>,
    relay_batch_subscribes: Counter<u64>,
    relay_batch_subscribe_failures: Counter<u64>,
    relay_batch_subscribe_latency: Histogram<u64>,
    relay_batch_subscribe_request_latency: Histogram<u64>,
    postgres_queries: Counter<u64>,
    postgres_query_latency: Histogram<u64>,
    keys_server_requests: Counter<u64>,
    keys_server_request_latency: Histogram<u64>,
    registry_requests: Counter<u64>,
    registry_request_latency: Histogram<u64>,
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
        const RELAY_OUTGOING_MESSAGE_LATENCY: &str = "relay_outgoing_message_latency";
        const RELAY_SUBSCRIBE_LATENCY: &str = "relay_subscribe_latency";
        const KEYS_SERVER_REQUEST_LATENCY: &str = "keys_server_request_latency";
        const REGISTRY_REQUEST_LATENCY: &str = "registry_request_latency";

        ServiceMetrics::init_with_name_and_meter_provider_builder("notify-server", |builder| {
            builder.with_view(|i: &Instrument| match &*i.name {
                RELAY_OUTGOING_MESSAGE_LATENCY | RELAY_SUBSCRIBE_LATENCY => Some(
                    Stream::new().aggregation(Aggregation::ExplicitBucketHistogram {
                        boundaries: vec![
                            0.0, 25.0, 50.0, 75.0, 100.0, 250.0, 500.0, 750.0, 1000.0, 2500.0,
                            5000.0, 7500.0, 10000.0,
                        ],
                        record_min_max: true,
                    }),
                ),
                KEYS_SERVER_REQUEST_LATENCY => Some(Stream::new().aggregation(
                    Aggregation::ExplicitBucketHistogram {
                        boundaries: vec![
                            0.0, 2.0, 5.0, 8.0, 10.0, 15.0, 20.0, 25.0, 30.0, 35.0, 40.0, 45.0,
                            50.0, 55.0, 60.0, 65.0, 70.0, 75.0, 100.0,
                        ],
                        record_min_max: true,
                    },
                )),
                REGISTRY_REQUEST_LATENCY => Some(Stream::new().aggregation(
                    Aggregation::ExplicitBucketHistogram {
                        boundaries: vec![
                            20.0, 50.0, 75.0, 100.0, 125.0, 150.0, 175.0, 200.0, 225.0, 250.0,
                            275.0, 300.0, 400.0,
                        ],
                        record_min_max: true,
                    },
                )),
                _ => None,
            })
        });

        let meter = ServiceMetrics::meter();

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
            .u64_histogram(RELAY_OUTGOING_MESSAGE_LATENCY)
            .with_description("The latency publishing relay messages w/ built-in retry")
            .init();

        let relay_outgoing_message_publish_latency: Histogram<u64> = meter
            .u64_histogram("relay_outgoing_message_publish_latency")
            .with_description("The latency publishing relay messages")
            .init();

        let relay_subscribes: Counter<u64> = meter
            .u64_counter("relay_subscribes")
            .with_description("The number of subscribes to relay topics (not including retries)")
            .init();

        let relay_subscribe_failures: Counter<u64> = meter
            .u64_counter("relay_subscribe_failures")
            .with_description("The number of failures to subscribe to relay topics")
            .init();

        let relay_subscribe_latency: Histogram<u64> = meter
            .u64_histogram(RELAY_SUBSCRIBE_LATENCY)
            .with_description("The latency subscribing to relay topics w/ built-in retry")
            .init();

        let relay_subscribe_request_latency: Histogram<u64> = meter
            .u64_histogram("relay_subscribe_request_latency")
            .with_description("The latency subscribing to relay topics")
            .init();

        let relay_batch_subscribes: Counter<u64> = meter
            .u64_counter("relay_batch_subscribes")
            .with_description(
                "The number of batch subscribes to relay topics (not including retries)",
            )
            .init();

        let relay_batch_subscribe_failures: Counter<u64> = meter
            .u64_counter("relay_batch_subscribe_failures")
            .with_description("The number of failures to batch subscribe to relay topics")
            .init();

        let relay_batch_subscribe_latency: Histogram<u64> = meter
            .u64_histogram("relay_batch_subscribe_latency")
            .with_description("The latency batch subscribing to relay topics w/ built-in retry")
            .init();

        let relay_batch_subscribe_request_latency: Histogram<u64> = meter
            .u64_histogram("relay_batch_subscribe_request_latency")
            .with_description("The latency batch subscribing to relay topics")
            .init();

        let postgres_queries: Counter<u64> = meter
            .u64_counter("postgres_queries")
            .with_description("The number of Postgres queries executed")
            .init();

        let postgres_query_latency: Histogram<u64> = meter
            .u64_histogram("postgres_query_latency")
            .with_description("The latency Postgres queries")
            .init();

        let keys_server_requests: Counter<u64> = meter
            .u64_counter("keys_server_requests")
            .with_description("The number of Keys Server requests")
            .init();

        let keys_server_request_latency: Histogram<u64> = meter
            .u64_histogram(KEYS_SERVER_REQUEST_LATENCY)
            .with_description("The latency Keys Server requests")
            .init();

        let registry_requests: Counter<u64> = meter
            .u64_counter("registry_requests")
            .with_description("The number of Registry requests")
            .init();

        let registry_request_latency: Histogram<u64> = meter
            .u64_histogram(REGISTRY_REQUEST_LATENCY)
            .with_description("The latency Registry requests")
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
            relay_subscribes,
            relay_subscribe_failures,
            relay_subscribe_latency,
            relay_subscribe_request_latency,
            relay_batch_subscribes,
            relay_batch_subscribe_failures,
            relay_batch_subscribe_latency,
            relay_batch_subscribe_request_latency,
            postgres_queries,
            postgres_query_latency,
            keys_server_requests,
            keys_server_request_latency,
            registry_requests,
            registry_request_latency,
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
    pub fn http_request(&self, endpoint: &str, method: &str, status: StatusCode, start: Instant) {
        let elapsed = start.elapsed();

        let attributes = [
            KeyValue::new("endpoint", endpoint.to_owned()),
            KeyValue::new("method", method.to_owned()),
            KeyValue::new("status", status.as_u16().to_string()),
        ];
        self.http_requests.add(1, &attributes);
        self.http_request_latency
            .record(elapsed.as_millis() as u64, &attributes);
    }

    pub fn relay_incoming_message(
        &self,
        tag: u32,
        status: RelayIncomingMessageStatus,
        start: Instant,
    ) {
        let elapsed = start.elapsed();

        let attributes = [
            KeyValue::new("tag", tag.to_string()),
            KeyValue::new("status", status.to_string()),
        ];
        self.relay_incoming_messages.add(1, &attributes);
        self.relay_incoming_message_latency
            .record(elapsed.as_millis() as u64, &attributes);
    }

    pub fn relay_outgoing_message(&self, tag: u32, success: bool, start: Instant) {
        let elapsed = start.elapsed();

        let attributes = [
            KeyValue::new("tag", tag.to_string()),
            KeyValue::new("success", success.to_string()),
        ];
        self.relay_outgoing_messages.add(1, &attributes);
        self.relay_outgoing_message_latency
            .record(elapsed.as_millis() as u64, &attributes);
    }

    pub fn relay_outgoing_message_failure(&self, tag: u32, is_permanent: bool) {
        let attributes = [
            KeyValue::new("tag", tag.to_string()),
            KeyValue::new("is_permanent", is_permanent.to_string()),
        ];
        self.relay_outgoing_message_failures.add(1, &attributes);
    }

    pub fn relay_outgoing_message_publish(&self, tag: u32, start: Instant) {
        let elapsed = start.elapsed();

        let attributes = [KeyValue::new("tag", tag.to_string())];
        self.relay_outgoing_message_publish_latency
            .record(elapsed.as_millis() as u64, &attributes);
    }

    pub fn relay_subscribe(&self, success: bool, start: Instant) {
        let elapsed = start.elapsed();

        let attributes = [KeyValue::new("success", success.to_string())];
        self.relay_subscribes.add(1, &attributes);
        self.relay_subscribe_latency
            .record(elapsed.as_millis() as u64, &attributes);
    }

    pub fn relay_subscribe_failure(&self, is_permanent: bool) {
        let attributes = [KeyValue::new("is_permanent", is_permanent.to_string())];
        self.relay_subscribe_failures.add(1, &attributes);
    }

    pub fn relay_subscribe_request(&self, start: Instant) {
        let elapsed = start.elapsed();

        self.relay_subscribe_request_latency
            .record(elapsed.as_millis() as u64, &[]);
    }

    pub fn relay_batch_subscribe(&self, success: bool, start: Instant) {
        let elapsed = start.elapsed();

        let attributes = [KeyValue::new("success", success.to_string())];
        self.relay_batch_subscribes.add(1, &attributes);
        self.relay_batch_subscribe_latency
            .record(elapsed.as_millis() as u64, &attributes);
    }

    pub fn relay_batch_subscribe_failure(&self, is_permanent: bool) {
        let attributes = [KeyValue::new("is_permanent", is_permanent.to_string())];
        self.relay_batch_subscribe_failures.add(1, &attributes);
    }

    pub fn relay_batch_subscribe_request(&self, start: Instant) {
        let elapsed = start.elapsed();

        self.relay_batch_subscribe_request_latency
            .record(elapsed.as_millis() as u64, &[]);
    }

    pub fn postgres_query(&self, query_name: &'static str, start: Instant) {
        let elapsed = start.elapsed();

        let attributes = [KeyValue::new("name", query_name)];
        self.postgres_queries.add(1, &attributes);
        self.postgres_query_latency
            .record(elapsed.as_millis() as u64, &attributes);
    }

    pub fn keys_server_request(&self, start: Instant, source: &KeysServerResponseSource) {
        let elapsed = start.elapsed();

        self.keys_server_requests
            .add(1, &[otel::KeyValue::new("source", source.as_str())]);
        self.keys_server_request_latency.record(
            elapsed.as_millis() as u64,
            &[otel::KeyValue::new("source", source.as_str())],
        );
    }

    pub fn registry_request(&self, start: Instant) {
        let elapsed = start.elapsed();

        self.registry_requests.add(1, &[]);
        self.registry_request_latency
            .record(elapsed.as_millis() as u64, &[]);
    }
}

pub async fn http_request_middleware(
    State(state): State<Arc<AppState>>,
    path: MatchedPath,
    method: Method,
    request: Request,
    next: Next,
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
    ClientError,
    ServerError,
}

impl Display for RelayIncomingMessageStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Success => write!(f, "success"),
            Self::ClientError => write!(f, "client_error"),
            Self::ServerError => write!(f, "server_error"),
        }
    }
}
