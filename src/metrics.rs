use {
    crate::error::{Error, Result},
    opentelemetry::{
        metrics::{Counter, Histogram, UpDownCounter},
        sdk::{
            self,
            export::metrics::aggregation,
            metrics::{processors, selectors},
            Resource,
        },
    },
    opentelemetry_prometheus::PrometheusExporter,
    prometheus_core::TextEncoder,
};

#[derive(Clone)]
// TODO: Proper metrics
pub struct Metrics {
    pub prometheus_exporter: PrometheusExporter,
    pub registered_clients: UpDownCounter<i64>,
    pub dispatched_notifications: Counter<u64>,
    pub send_latency: Histogram<u64>,
}

impl Metrics {
    pub fn new(resource: Resource) -> Result<Self> {
        let controller = sdk::metrics::controllers::basic(
            processors::factory(
                selectors::simple::histogram(vec![]),
                aggregation::cumulative_temporality_selector(),
            )
            .with_memory(true),
        )
        .with_resource(resource)
        .build();

        let prometheus_exporter = opentelemetry_prometheus::exporter(controller).init();

        let meter = prometheus_exporter.meter_provider().unwrap();

        opentelemetry::global::set_meter_provider(meter);

        let meter = opentelemetry::global::meter("cast-server");

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

        Ok(Metrics {
            prometheus_exporter,
            registered_clients: clients_counter,
            dispatched_notifications,
            send_latency,
        })
    }

    pub fn export(&self) -> Result<String> {
        let data = self.prometheus_exporter.registry().gather();
        TextEncoder::new()
            .encode_to_string(&data)
            .map_err(Error::Prometheus)
    }
}
