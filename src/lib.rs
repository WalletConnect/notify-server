pub mod config;
pub mod error;
mod handlers;
mod state;
mod stores;

use {
    crate::{
        config::Configuration,
        state::{AppState, Metrics},
    },
    axum::{routing::get, Router},
    log::LevelFilter,
    opentelemetry::{
        sdk::{
            metrics::selectors,
            trace::{self, IdGenerator, Sampler},
            Resource,
        },
        util::tokio_interval_stream,
        KeyValue,
    },
    opentelemetry_otlp::{Protocol, WithExportConfig},
    sqlx::{
        postgres::{PgConnectOptions, PgPoolOptions},
        ConnectOptions,
    },
    std::{net::SocketAddr, str::FromStr, sync::Arc, time::Duration},
    tokio::{select, sync::broadcast},
    tower::ServiceBuilder,
    tracing::info,
    tracing_subscriber::fmt::format::FmtSpan,
};

build_info::build_info!(fn build_info);

pub type Result<T> = std::result::Result<T, error::Error>;

pub async fn bootstap(mut shutdown: broadcast::Receiver<()>, config: Configuration) -> Result<()> {
    let pg_options = PgConnectOptions::from_str(&config.database_url)?
        .log_statements(LevelFilter::Debug)
        .log_slow_statements(LevelFilter::Info, Duration::from_millis(250))
        .clone();

    let store = PgPoolOptions::new()
        .max_connections(5)
        .connect_with(pg_options)
        .await?;

    sqlx::migrate!("./migrations").run(&store).await?;

    let mut state = AppState::new(config, Arc::new(store.clone()))?;

    // Telemetry
    if state.config.telemetry_enabled.unwrap_or(false) {
        let grpc_url = state
            .config
            .telemetry_grpc_url
            .clone()
            .unwrap_or_else(|| "http://localhost:4317".to_string());

        let tracing_exporter = opentelemetry_otlp::new_exporter()
            .tonic()
            .with_endpoint(grpc_url.clone())
            .with_timeout(Duration::from_secs(5))
            .with_protocol(Protocol::Grpc);

        let tracer = opentelemetry_otlp::new_pipeline()
            .tracing()
            .with_exporter(tracing_exporter)
            .with_trace_config(
                trace::config()
                    .with_sampler(Sampler::AlwaysOn)
                    .with_id_generator(IdGenerator::default())
                    .with_max_events_per_span(64)
                    .with_max_attributes_per_span(16)
                    .with_max_events_per_span(16)
                    .with_resource(Resource::new(vec![
                        KeyValue::new("service.name", state.build_info.crate_info.name.clone()),
                        KeyValue::new(
                            "service.version",
                            state.build_info.crate_info.version.clone().to_string(),
                        ),
                    ])),
            )
            .install_batch(opentelemetry::runtime::Tokio)?;

        let metrics_exporter = opentelemetry_otlp::new_exporter()
            .tonic()
            .with_endpoint(grpc_url)
            .with_timeout(Duration::from_secs(5))
            .with_protocol(Protocol::Grpc);

        let meter_provider = opentelemetry_otlp::new_pipeline()
            .metrics(tokio::spawn, tokio_interval_stream)
            .with_exporter(metrics_exporter)
            .with_period(Duration::from_secs(3))
            .with_timeout(Duration::from_secs(10))
            .with_aggregator_selector(selectors::simple::Selector::Exact)
            .build()?;

        opentelemetry::global::set_meter_provider(meter_provider.provider());

        let meter = opentelemetry::global::meter("cast-server");
        let example_counter = meter
            .i64_up_down_counter("example")
            .with_description("This is an example counter")
            .init();

        state.set_telemetry(tracer, Metrics {
            example: example_counter,
        })
    } else if !state.config.is_test {
        // Only log to console if telemetry disabled
        tracing_subscriber::fmt()
            .with_max_level(state.config.log_level())
            .with_span_events(FmtSpan::CLOSE)
            .init();
    }

    let port = state.config.port;

    let state_arc = Arc::new(state);

    let global_middleware = ServiceBuilder::new();

    let app = Router::new()
        .route("/health", get(handlers::health::handler))
        .layer(global_middleware)
        .with_state(state_arc);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    select! {
        _ = axum::Server::bind(&addr).serve(app.into_make_service()) => info!("Server terminating"),
        _ = shutdown.recv() => info!("Shutdown signal received, killing servers"),
    }

    Ok(())
}
