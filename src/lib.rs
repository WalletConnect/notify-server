use {
    crate::{
        config::Configuration,
        metrics::Metrics,
        state::AppState,
        websocket_service::WebsocketService,
    },
    axum::{
        http,
        routing::{delete, get, post, put},
        Router,
    },
    mongodb::options::{ClientOptions, ResolverConfig},
    rand::prelude::*,
    relay_rpc::auth::ed25519_dalek::Keypair,
    std::{net::SocketAddr, sync::Arc},
    tokio::{select, sync::broadcast},
    tower::ServiceBuilder,
    tower_http::{
        cors::{Any, CorsLayer},
        trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer},
    },
    tracing::{info, Level},
};
pub mod analytics;
pub mod auth;
pub mod config;
pub mod error;
pub mod extractors;
pub mod handlers;
pub mod jsonrpc;
mod metrics;
mod networking;
mod notify_keys;
pub mod registry;
pub mod spec;
mod state;
mod storage;
pub mod types;
pub mod websocket_service;
pub mod wsclient;

#[macro_use]
extern crate lazy_static;

build_info::build_info!(fn build_info);

pub type Result<T> = std::result::Result<T, error::Error>;

pub async fn bootstap(mut shutdown: broadcast::Receiver<()>, config: Configuration) -> Result<()> {
    wc::metrics::ServiceMetrics::init_with_name("notify-server");
    let analytics = analytics::initialize(&config).await?;

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
            .database("notify"),
    );

    let seed = sha256::digest(config.keypair_seed.as_bytes()).as_bytes()[..32]
        .try_into()
        .map_err(|_| error::Error::InvalidKeypairSeed)?;
    let keypair = Keypair::generate(&mut StdRng::from_seed(seed));

    // Create a websocket client to communicate with relay
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

    let connection_handler = wsclient::RelayConnectionHandler::new("notify-client", tx);
    let wsclient = Arc::new(relay_client::websocket::Client::new(connection_handler));
    let http_client = Arc::new(create_http_client(
        &keypair,
        &config.relay_url.replace("ws", "http"),
        &config.notify_url,
        &config.project_id,
    ));

    let registry = Arc::new(registry::Registry::new(
        &config.registry_url,
        &config.registry_auth_token,
        &config,
    )?);

    // Creating state
    let state = AppState::new(
        analytics,
        config,
        db,
        keypair,
        wsclient.clone(),
        http_client,
        Some(Metrics::default()),
        registry,
    )?;

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
        .route("/:project_id/notify", post(handlers::notify::handler))
        .route(
            "/:project_id/subscribe-topic",
            post(handlers::subscribe_topic::handler),
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
        .route("/.well-known/did.json", get(handlers::did_json::handler))
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

fn create_http_client(
    key: &Keypair,
    http_relay_url: &str,
    notify_url: &str,
    project_id: &str,
) -> relay_client::http::Client {
    let rpc_address = format!("{http_relay_url}/rpc");
    let aud_address = http_relay_url.to_string();

    let auth = relay_rpc::auth::AuthToken::new(notify_url)
        .aud(aud_address)
        .as_jwt(key)
        .unwrap();

    let conn_opts =
        relay_client::ConnectionOptions::new(project_id, auth).with_address(rpc_address);

    relay_client::http::Client::new(&conn_opts).unwrap()
}
