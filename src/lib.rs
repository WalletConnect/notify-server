use {
    crate::{
        config::Configuration,
        error::Result,
        metrics::Metrics,
        registry::storage::redis::Redis,
        relay_client_helpers::create_http_client,
        services::{
            private_http_server, public_http_server, publisher_service, watcher_expiration_job,
            websocket_server::{self, decode_key},
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
pub mod notify_message;
pub mod publish_relay_message;
pub mod rate_limit;
pub mod registry;
pub mod relay_client_helpers;
pub mod services;
pub mod spec;
pub mod state;
pub mod types;
pub mod utils;

build_info::build_info!(fn build_info);

pub async fn bootstrap(mut shutdown: broadcast::Receiver<()>, config: Configuration) -> Result<()> {
    wc::metrics::ServiceMetrics::init_with_name("notify-server");

    let s3_client = get_s3_client(&config).await;
    let geoip_resolver = get_geoip_resolver(&config, &s3_client).await;

    let analytics = analytics::initialize(&config, s3_client, geoip_resolver.clone()).await?;

    let postgres = PgPoolOptions::new()
        // https://docs.aws.amazon.com/AmazonRDS/latest/AuroraUserGuide/aurora-serverless-v2.setting-capacity.html#aurora-serverless-v2.max-connections
        .max_connections(config.postgres_max_connections)
        .connect(&config.postgres_url).await?;
    sqlx::migrate!("./migrations").run(&postgres).await?;

    let keypair_seed = decode_key(&sha256::digest(config.keypair_seed.as_bytes()))
        .map_err(|_| error::Error::InvalidKeypairSeed)?;
    let keypair = Keypair::generate(&mut StdRng::from_seed(keypair_seed));

    let (relay_ws_client, rx) = {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let connection_handler =
            services::websocket_server::relay_ws_client::RelayConnectionHandler::new(
                "notify-client",
                tx,
            );
        let relay_ws_client = Arc::new(relay_client::websocket::Client::new(connection_handler));
        (relay_ws_client, rx)
    };

    let relay_http_client = Arc::new(create_http_client(
        &keypair,
        config.relay_url.clone(),
        config.notify_url.clone(),
        config.project_id.clone(),
    )?);

    let metrics = Some(Metrics::default());

    let redis = if let Some(redis_addr) = &config.auth_redis_addr() {
        Some(Arc::new(Redis::new(
            redis_addr,
            config.redis_pool_size as usize,
        )?))
    } else {
        None
    };

    let registry = Arc::new(registry::Registry::new(
        config.registry_url.clone(),
        &config.registry_auth_token,
        redis.clone(),
        metrics.clone(),
    )?);

    let state = Arc::new(AppState::new(
        analytics.clone(),
        config.clone(),
        postgres.clone(),
        keypair,
        keypair_seed,
        relay_ws_client.clone(),
        relay_http_client.clone(),
        metrics.clone(),
        redis,
        registry,
        config.clock,
    )?);

    let private_http_server =
        private_http_server::start(config.bind_ip, config.telemetry_prometheus_port);
    let public_http_server = public_http_server::start(
        config.bind_ip,
        config.port,
        config.blocked_countries,
        state.clone(),
        geoip_resolver,
    );
    let websocket_server = websocket_server::start(state, relay_ws_client, rx);
    let publisher_service = publisher_service::start(
        postgres.clone(),
        relay_http_client.clone(),
        metrics.clone(),
        analytics,
    );
    let watcher_expiration_job = watcher_expiration_job::start(postgres, metrics);

    select! {
        _ = shutdown.recv() => info!("Shutdown signal received, killing services"),
        e = private_http_server => error!("Private HTTP server terminating with error {e:?}"),
        e = public_http_server => error!("Public HTTP server terminating with error {e:?}"),
        e = websocket_server => error!("Relay websocket server terminating with error {e:?}"),
        e = publisher_service => error!("Publisher service terminating with error {e:?}"),
        e = watcher_expiration_job => error!("Watcher expiration job terminating with error {e:?}"),
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
