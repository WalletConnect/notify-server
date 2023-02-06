use {
    crate::Configuration,
    build_info::BuildInfo,
    ed25519_dalek::Keypair,
    opentelemetry::{metrics::UpDownCounter, sdk::trace::Tracer},
    std::sync::Arc,
    tracing_subscriber::prelude::*,
};

#[derive(Clone)]
pub struct Metrics {
    pub example: UpDownCounter<i64>,
}

// #[derive(Clone)]
pub struct AppState {
    pub config: Configuration,
    pub build_info: BuildInfo,
    pub metrics: Option<Metrics>,
    pub example_store: Arc<mongodb::Database>, // ExampleStoreArc,
    pub keypair: Keypair,
}

build_info::build_info!(fn build_info);

impl AppState {
    pub fn new(
        config: Configuration,
        example_store: Arc<mongodb::Database>, // ExampleStoreArc
        keypair: Keypair,
    ) -> crate::Result<AppState> {
        let build_info: &BuildInfo = build_info();

        Ok(AppState {
            config,
            build_info: build_info.clone(),
            metrics: None,
            example_store,
            keypair,
        })
    }

    pub fn set_telemetry(&mut self, tracer: Tracer, metrics: Metrics) {
        let otel_tracing_layer = tracing_opentelemetry::layer().with_tracer(tracer);

        tracing_subscriber::registry()
            .with(otel_tracing_layer)
            .init();

        self.metrics = Some(metrics);
    }
}
