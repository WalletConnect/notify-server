use {
    super::{config::Configuration, error::Error, Result},
    axum::{routing::get, Router},
    rand::prelude::*,
    relay_rpc::auth::ed25519_dalek::Keypair,
    sqlx::postgres::PgPoolOptions,
    std::{net::SocketAddr, sync::Arc},
    tokio::select,
    tracing::{info, warn},
};

pub mod helpers;
pub mod metrics;
pub mod types;
pub mod worker;

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

pub async fn bootstrap(config: Configuration) -> Result<()> {
    wc::metrics::ServiceMetrics::init_with_name("notify-publisher-service");

    let postgres_pool = PgPoolOptions::new().connect(&config.postgres_url).await?;
    sqlx::migrate!("./migrations").run(&postgres_pool).await?;

    let seed = sha256::digest(config.keypair_seed.as_bytes()).as_bytes()[..32]
        .try_into()
        .map_err(|_| Error::InvalidKeypairSeed)?;
    let keypair = Keypair::generate(&mut StdRng::from_seed(seed));
    let http_relay_client = Arc::new(create_http_client(
        &keypair,
        &config.relay_url.replace("ws", "http"),
        &config.notify_url,
        &config.project_id,
    ));
    let telemetry_port = config.telemetry_prometheus_port.unwrap_or(3001);
    let telemetry_addr = SocketAddr::from(([0, 0, 0, 0], telemetry_port));

    info!("Starting metrics server on {}", telemetry_addr);
    let telemetry_app = Router::new().route("/metrics", get(metrics::handler));
    let postgres_pool_arc = Arc::new(postgres_pool);
    select! {
        e = axum::Server::bind(&telemetry_addr).serve(telemetry_app.into_make_service()) => warn!("Metrics server terminated {:?}", e),
        e = worker::run(&postgres_pool_arc, &http_relay_client) => warn!("Worker process terminated {:?}", e),
    }

    Ok(())
}
