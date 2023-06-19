use {
    crate::{
        config::Configuration,
        log::info,
        state::AppState,
        websocket_service::WebsocketService,
    },
    axum::{
        http,
        routing::{delete, get, post, put},
        Router,
    },
    mongodb::options::{ClientOptions, ResolverConfig},
    opentelemetry::{sdk::Resource, KeyValue},
    rand::prelude::*,
    relay_rpc::auth::ed25519_dalek::Keypair,
    std::{net::SocketAddr, sync::Arc, time::Duration},
    tokio::{select, sync::broadcast},
    tower::ServiceBuilder,
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
pub mod types;
pub mod websocket_service;
pub mod wsclient;

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

    // Create a websocket client to communicate with relay
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

    let connection_handler = wsclient::RelayConnectionHandler::new("cast-client", tx);
    let wsclient = Arc::new(relay_client::websocket::Client::new(connection_handler));
    let http_client = Arc::new(create_http_client(
        &keypair,
        &config.relay_url.replace("ws", "http"),
        &config.cast_url,
        &config.project_id,
    ));

    // Creating state
    let mut state = AppState::new(config, db, keypair, wsclient.clone(), http_client)?;

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
        .allow_headers([http::header::CONTENT_TYPE, http::header::AUTHORIZATION]);

    let app = Router::new()
        .route("/health", get(handlers::health::handler))
        .route("/:project_id/register", post(handlers::register::handler))
        .route("/:project_id/notify", post(handlers::notify::handler))
        .route(
            "/:project_id/subscribe-topic",
            get(handlers::subscribe_topic::handler),
        )
        .route(
            "/:project_id/register-webhook",
            post(handlers::webhooks::register_webhook::handler),
        )
        .route(
            "/:project_id/webhooks",
            get(handlers::webhooks::get_webhooks::handler),
        )
        .route(
            "/:project_id/webhooks/:webhook_id",
            delete(handlers::webhooks::delete_webhook::handler),
        )
        .route(
            "/:project_id/webhooks/:webhook_id",
            put(handlers::webhooks::update_webhook::handler),
        )
        .route(
            "/:project_id/subscribers",
            get(handlers::get_subscribers::handler),
        )
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

    // Start the websocket service
    info!("Starting websocket service");
    let mut websocket_service = WebsocketService::new(state_arc, wsclient, rx).await?;

    select! {
        _ = axum::Server::bind(&private_addr).serve(private_app.into_make_service()) => info!("Terminating metrics service"),
        _ = axum::Server::bind(&addr).serve(app.into_make_service_with_connect_info::<SocketAddr>()) => info!("Server terminating"),
        _ = shutdown.recv() => info!("Shutdown signal received, killing servers"),
        e = websocket_service.run() => info!("Unregister service terminating {:?}", e),
    }

    Ok(())
}

// TODO: This is 50 years, only used temporarely untill the client is changed.
const HTTP_CLIENT_TTL: u64 = 50 * 365 * 24 * 60 * 60;

fn create_http_client(
    key: &Keypair,
    http_relay_url: &str,
    cast_url: &str,
    project_id: &str,
) -> relay_client::http::Client {
    let rpc_address = format!("{http_relay_url}/rpc");
    let aud_address = http_relay_url.to_string();

    let auth = relay_rpc::auth::AuthToken::new(cast_url)
        .aud(aud_address)
        .ttl(Duration::from_secs(HTTP_CLIENT_TTL))
        .as_jwt(key)
        .unwrap();

    let conn_opts =
        relay_client::ConnectionOptions::new(project_id, auth).with_address(rpc_address);

    relay_client::http::Client::new(&conn_opts).unwrap()
}
