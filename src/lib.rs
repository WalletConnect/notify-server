use {
    axum::{http, routing::post},
    mongodb::options::{ClientOptions, ResolverConfig},
    rand::prelude::*,
    tower_http::{
        cors::{Any, CorsLayer},
        trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer},
    },
    tracing::Level,
};

pub mod auth;
pub mod config;
pub mod error;
pub mod handlers;
pub mod jsonrpc;
pub mod log;
mod metrics;
mod state;
pub mod wsclient;

use {
    crate::{config::Configuration, state::AppState},
    axum::{routing::get, Router},
    opentelemetry::{
        sdk::Resource,
        // util::tokio_interval_stream,
        KeyValue,
    },
    std::{net::SocketAddr, sync::Arc},
    tokio::{select, sync::broadcast},
    tower::ServiceBuilder,
    tracing::info,
};

build_info::build_info!(fn build_info);

pub type Result<T> = std::result::Result<T, error::Error>;

pub async fn bootstap(mut shutdown: broadcast::Receiver<()>, config: Configuration) -> Result<()> {
    // A Client is needed to connect to MongoDB:
    // An extra line of code to work around a DNS issue on Windows:
    let options = ClientOptions::parse_with_resolver_config(
        &config.database_url,
        ResolverConfig::cloudflare(),
    )
    .await?;

    let db = Arc::new(
        mongodb::Client::with_options(options)
            .unwrap()
            .database("cast"),
    );

    let seed: [u8; 32] = config.keypair_seed.as_bytes()[..32]
        .try_into()
        .map_err(|_| error::Error::InvalidKeypairSeed)?;
    let mut seeded = StdRng::from_seed(seed);
    let keypair = ed25519_dalek::Keypair::generate(&mut seeded);

    // Creating state
    let mut state = AppState::new(config, db, keypair)?; //, Arc::new(store.clone()))?;

    // Telemetry
    if state.config.telemetry_prometheus_port.is_some() {
        state.set_metrics(metrics::Metrics::new(Resource::new(vec![
            KeyValue::new("service_name", "cast-server"),
            KeyValue::new(
                "service_version",
                state.build_info.crate_info.version.clone().to_string(),
            ),
        ]))?);
    }

    let port = state.config.port;

    let state_arc = Arc::new(state);

    let global_middleware = ServiceBuilder::new().layer(
        TraceLayer::new_for_http()
            .make_span_with(DefaultMakeSpan::new().include_headers(true))
            .on_request(DefaultOnRequest::new().level(Level::INFO))
            .on_response(
                DefaultOnResponse::new()
                    .level(Level::INFO)
                    .include_headers(true),
            ),
    );

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_headers([http::header::CONTENT_TYPE]);

    let app = Router::new()
        .route("/health", get(handlers::health::handler))
        .route("/:project_id/register", post(handlers::register::handler))
        .route("/:project_id/notify", post(handlers::notify::handler))
        .layer(global_middleware)
        .layer(cors)
        .with_state(state_arc.clone());

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let private_addr = SocketAddr::from((
        [0, 0, 0, 0],
        state_arc.config.telemetry_prometheus_port.unwrap_or(3001),
    ));

    let private_app = Router::new()
        .route("/metrics", get(handlers::metrics::handler))
        .with_state(state_arc);

    select! {
        _ = axum::Server::bind(&private_addr).serve(private_app.into_make_service()) => info!("Terminating metrics service"),
        _ = axum::Server::bind(&addr).serve(app.into_make_service()) => info!("Server terminating"),
        _ = shutdown.recv() => info!("Shutdown signal received, killing servers"),
    }

    Ok(())
}
