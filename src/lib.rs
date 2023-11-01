use {
    crate::{
        config::Configuration,
        metrics::Metrics,
        relay_client_helpers::create_http_client,
        services::{
            private_http, public_http,
            watcher_expiration::watcher_expiration_job,
            websocket_service::{decode_key, WebsocketService},
        },
        state::AppState,
    },
    aws_config::meta::region::RegionProviderChain,
    aws_sdk_s3::{config::Region, Client as S3Client},
    rand::prelude::*,
    relay_rpc::auth::ed25519_dalek::Keypair,
    sqlx::postgres::PgPoolOptions,
    std::sync::Arc,
    tokio::{select, sync::broadcast},
    tracing::{error, info},
    wc::geoip::MaxMindResolver,
};

#[macro_use]
extern crate lazy_static;

pub mod analytics;
pub mod auth;
pub mod config;
pub mod error;
pub mod jsonrpc;
mod metrics;
pub mod model;
mod notify_keys;
pub mod registry;
pub mod relay_client_helpers;
pub mod services;
pub mod spec;
pub mod state;
pub mod types;

build_info::build_info!(fn build_info);

pub type Result<T> = std::result::Result<T, error::Error>;

pub async fn bootstrap(mut shutdown: broadcast::Receiver<()>, config: Configuration) -> Result<()> {
    wc::metrics::ServiceMetrics::init_with_name("notify-server");

    let s3_client = get_s3_client(&config).await;
    let geoip_resolver = get_geoip_resolver(&config, &s3_client).await;

    let analytics = analytics::initialize(&config, s3_client, geoip_resolver.clone()).await?;

    let postgres = PgPoolOptions::new().connect(&config.postgres_url).await?;
    sqlx::migrate!("./migrations").run(&postgres).await?;

    let keypair_seed = decode_key(&sha256::digest(config.keypair_seed.as_bytes()))
        .map_err(|_| error::Error::InvalidKeypairSeed)?;
    let keypair = Keypair::generate(&mut StdRng::from_seed(keypair_seed));

    // Create a websocket client to communicate with relay
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

    let connection_handler =
        services::websocket_service::wsclient::RelayConnectionHandler::new("notify-client", tx);
    let wsclient = Arc::new(relay_client::websocket::Client::new(connection_handler));
    let http_client = Arc::new(create_http_client(
        &keypair,
        config.relay_url.clone(),
        config.notify_url.clone(),
        config.project_id.clone(),
    )?);

    let registry = Arc::new(registry::Registry::new(
        &config.registry_url,
        &config.registry_auth_token,
        &config,
    )?);

    let state = Arc::new(AppState::new(
        analytics,
        config.clone(),
        postgres.clone(),
        keypair,
        keypair_seed,
        wsclient.clone(),
        http_client,
        Some(Metrics::default()),
        registry,
    )?);

    // Start the websocket service
    info!("Starting websocket service");
    let mut websocket_service = WebsocketService::new(state.clone(), wsclient, rx).await?;

    select! {
        _ = private_http::start(config.telemetry_prometheus_port) => info!("Private HTTP server terminating"),
        _ = public_http::start(config.port,  config.blocked_countries, state, geoip_resolver) => info!("Public HTTP server terminating"),
        _ = shutdown.recv() => info!("Shutdown signal received, killing servers"),
        e = websocket_service.run() => info!("Websocket service terminating {:?}", e),
        e = watcher_expiration_job(postgres) => info!("Watcher expiration job terminating {:?}", e),
    }

    Ok(())
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
