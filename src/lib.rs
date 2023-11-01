#[macro_use]
extern crate lazy_static;

use {
    crate::{
        config::Configuration, metrics::Metrics, state::AppState,
        watcher_expiration::watcher_expiration_job, websocket_service::WebsocketService,
    },
    aws_config::meta::region::RegionProviderChain,
    aws_sdk_s3::{config::Region, Client as S3Client},
    axum::{
        http,
        routing::{get, post},
        Router,
    },
    rand::prelude::*,
    relay_rpc::auth::ed25519_dalek::Keypair,
    sqlx::postgres::PgPoolOptions,
    std::{net::SocketAddr, sync::Arc},
    tokio::{select, sync::broadcast},
    tower::ServiceBuilder,
    tower_http::{
        cors::{Any, CorsLayer},
        trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer},
    },
    tracing::{error, info, Level},
    wc::geoip::{
        block::{middleware::GeoBlockLayer, BlockingPolicy},
        MaxMindResolver,
    },
};

pub mod analytics;
pub mod auth;
pub mod config;
pub mod error;
pub mod extractors;
pub mod handlers;
pub mod jsonrpc;
mod metrics;
pub mod migrate;
pub mod model;
mod networking;
mod notify_keys;
pub mod publisher_service;
pub mod registry;
pub mod spec;
pub mod state;
mod storage;
pub mod types;
pub mod watcher_expiration;
pub mod websocket_service;
pub mod wsclient;

build_info::build_info!(fn build_info);

pub type Result<T> = std::result::Result<T, error::Error>;

pub async fn bootstrap(mut shutdown: broadcast::Receiver<()>, config: Configuration) -> Result<()> {
    wc::metrics::ServiceMetrics::init_with_name("notify-server");

    let s3_client = get_s3_client(&config).await;
    let geoip_resolver = get_geoip_resolver(&config, &s3_client).await;

    let analytics = analytics::initialize(&config, s3_client, geoip_resolver.clone()).await?;

    let postgres = PgPoolOptions::new().connect(&config.postgres_url).await?;
    sqlx::migrate!("./migrations").run(&postgres).await?;

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
        postgres,
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
        .route("/.well-known/did.json", get(handlers::did_json::handler))
        .route("/:project_id/notify", post(handlers::notify_v0::handler))
        .route("/v1/:project_id/notify", post(handlers::notify_v1::handler))
        .route(
            "/:project_id/subscribers",
            get(handlers::get_subscribers_v0::handler),
        )
        .route(
            "/v1/:project_id/subscribers",
            get(handlers::get_subscribers_v1::handler),
        )
        .route(
            "/:project_id/subscribe-topic",
            post(handlers::subscribe_topic::handler),
        )
        // FIXME
        // .route(
        //     "/:project_id/register-webhook",
        //     post(handlers::webhooks::register_webhook::handler),
        // )
        // .route(
        //     "/:project_id/webhooks",
        //     get(handlers::webhooks::get_webhooks::handler),
        // )
        // .route(
        //     "/:project_id/webhooks/:webhook_id",
        //     delete(handlers::webhooks::delete_webhook::handler),
        // )
        // .route(
        //     "/:project_id/webhooks/:webhook_id",
        //     put(handlers::webhooks::update_webhook::handler),
        // )
        .layer(global_middleware)
        .layer(cors);
    let app = if let Some(resolver) = geoip_resolver {
        app.layer(GeoBlockLayer::new(
            resolver.clone(),
            state_arc.config.blocked_countries.clone(),
            BlockingPolicy::AllowAll,
        ))
    } else {
        app
    };
    let app = app.with_state(state_arc.clone());

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
    let mut websocket_service = WebsocketService::new(state_arc.clone(), wsclient, rx).await?;

    select! {
        _ = axum::Server::bind(&private_addr).serve(private_app.into_make_service()) => info!("Private server terminating"),
        _ = axum::Server::bind(&addr).serve(app.into_make_service_with_connect_info::<SocketAddr>()) => info!("Server terminating"),
        _ = shutdown.recv() => info!("Shutdown signal received, killing servers"),
        e = websocket_service.run() => info!("Websocket service terminating {:?}", e),
        e = watcher_expiration_job(state_arc.clone()) => info!("Watcher expiration job terminating {:?}", e),
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

async fn get_s3_client(config: &Configuration) -> S3Client {
    let region_provider = RegionProviderChain::first_try(Region::new("eu-central-1"));
    let shared_config = aws_config::from_env().region(region_provider).load().await;

    let aws_config = match &config.s3_endpoint {
        Some(s3_endpoint) => {
            info!(%s3_endpoint, "initializing analytics with custom s3 endpoint");

            aws_sdk_s3::config::Builder::from(&shared_config)
                .endpoint_url(s3_endpoint)
                .build()
        }
        _ => aws_sdk_s3::config::Builder::from(&shared_config).build(),
    };

    S3Client::from_conf(aws_config)
}

async fn get_geoip_resolver(
    config: &Configuration,
    s3_client: &S3Client,
) -> Option<Arc<MaxMindResolver>> {
    match (&config.geoip_db_bucket, &config.geoip_db_key) {
        (Some(bucket), Some(key)) => {
            info!(%bucket, %key, "initializing geoip database from aws s3");

            MaxMindResolver::from_aws_s3(s3_client, bucket, key)
                .await
                .map_err(|err| {
                    error!(?err, "failed to load geoip resolver");
                    err
                })
                .ok()
                .map(Arc::new)
        }
        _ => {
            info!("geoip lookup is disabled");

            None
        }
    }
}
