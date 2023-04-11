use {
    crate::unregister_service::UnregisterService,
    axum::{http, routing::post},
    mongodb::options::{ClientOptions, ResolverConfig},
    rand::prelude::*,
    tower_http::{
        cors::{Any, CorsLayer},
        trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer},
    },
    tracing::Level,
    walletconnect_sdk::rpc::auth::ed25519_dalek::Keypair,
};

pub mod analytics;
pub mod auth;
pub mod config;
pub mod error;
pub mod handlers;
pub mod jsonrpc;
pub mod log;
mod metrics;
mod state;
pub mod types;
mod unregister_service;
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

    let mut seed: [u8; 32] = config.keypair_seed.as_bytes()[..32]
        .try_into()
        .map_err(|_| error::Error::InvalidKeypairSeed)?;
    let mut seeded = StdRng::from_seed(seed);

    let keypair = Keypair::generate(&mut seeded);
    seed.reverse();
    let unregister_keypair = Keypair::generate(&mut seeded);

    // Create a channel for the unregister service
    let (unregister_tx, unregister_rx) = tokio::sync::mpsc::channel(100);

    // Creating state
    let mut state = AppState::new(config, db, keypair, unregister_keypair, unregister_tx)?;

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
    info!("Starting server on {}", addr);

    let private_port = state_arc.config.telemetry_prometheus_port.unwrap_or(3001);
    let private_addr = SocketAddr::from(([0, 0, 0, 0], private_port));

    info!("Starting metric server on {}", private_addr);

    let private_app = Router::new()
        .route("/metrics", get(handlers::metrics::handler))
        .with_state(state_arc.clone());

    // Start the unregister service
    info!("Starting unregister service");
    let mut unregister_service = UnregisterService::new(state_arc, unregister_rx).await?;

    select! {
        // TODO: change bind to try_bind, handleable errrors
        // If possible to retrieve port from `from_tcp` we may be able to finally get rid of flaky tests )
        _ = axum::Server::bind(&private_addr).serve(private_app.into_make_service()) => info!("Terminating metrics service"),
        _ = axum::Server::bind(&addr).serve(app.into_make_service_with_connect_info::<SocketAddr>()) => info!("Server terminating"),
        _ = shutdown.recv() => info!("Shutdown signal received, killing servers"),
        _ = unregister_service.run() => info!("Unregister service terminating"),
    }

    Ok(())
}
