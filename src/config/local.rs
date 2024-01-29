use {
    super::Configuration,
    crate::error::NotifyServerError,
    dotenvy::dotenv,
    relay_rpc::domain::ProjectId,
    serde::Deserialize,
    std::net::{IpAddr, Ipv4Addr, SocketAddr},
    url::Url,
};

// Configuration entrypoint for `cargo run`

#[derive(Deserialize, Debug)]
pub struct LocalConfiguration {
    pub project_id: ProjectId,
    pub registry_auth_token: String,

    #[serde(default = "default_bind_ip")]
    pub bind_ip: IpAddr,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_postgres_url")]
    pub postgres_url: String,
    #[serde(default = "default_postgres_max_connections")]
    pub postgres_max_connections: u32,
    #[serde(default = "default_redis_url")]
    pub redis_url: String,
    #[serde(default = "default_keypair_seed")]
    pub keypair_seed: String,
    #[serde(default = "default_relay_url")]
    pub relay_url: Url,
    #[serde(default = "default_registry_url")]
    pub registry_url: Url,
}

fn default_bind_ip() -> IpAddr {
    IpAddr::V4(Ipv4Addr::LOCALHOST)
}

fn default_port() -> u16 {
    3000
}

fn default_log_level() -> String {
    "WARN,notify_server=DEBUG".to_string()
}

pub fn default_postgres_url() -> String {
    "postgres://postgres:postgres@localhost:5432/postgres".to_owned()
}

pub fn default_redis_url() -> String {
    "redis://localhost:6378/0".to_owned()
}

pub fn default_postgres_max_connections() -> u32 {
    10
}

fn default_keypair_seed() -> String {
    hex::encode(rand::Rng::gen::<[u8; 10]>(&mut rand::thread_rng()))
}

fn default_relay_url() -> Url {
    "ws://127.0.0.1:8888".parse().unwrap()
}

fn default_registry_url() -> Url {
    "https://registry.walletconnect.com".parse().unwrap()
}

pub async fn get_configuration() -> Result<Configuration, NotifyServerError> {
    load_dot_env()?;
    let config = envy::from_env::<LocalConfiguration>()?;

    let relay_public_key = reqwest::get(config.relay_url.join("/public-key").unwrap())
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    let socket_addr = SocketAddr::from((config.bind_ip, config.port));
    let notify_url = format!("http://{socket_addr}").parse::<Url>().unwrap();
    let config = Configuration {
        public_ip: config.bind_ip,
        bind_ip: config.bind_ip,
        port: config.port,
        notify_url: notify_url.clone(),
        log_level: config.log_level,
        postgres_url: config.postgres_url,
        postgres_max_connections: config.postgres_max_connections,
        keypair_seed: config.keypair_seed,
        project_id: config.project_id,
        relay_url: config.relay_url,
        relay_public_key,
        registry_url: config.registry_url,
        registry_auth_token: config.registry_auth_token,
        auth_redis_addr_read: None,
        auth_redis_addr_write: None,
        redis_pool_size: 1,
        telemetry_prometheus_port: None,
        s3_endpoint: None,
        geoip_db_bucket: None,
        geoip_db_key: None,
        blocked_countries: vec![],
        analytics_export_bucket: None,
        clock: None,
    };

    Ok(config)
}

fn load_dot_env() -> dotenvy::Result<()> {
    match dotenv() {
        Ok(_) => Ok(()),
        Err(e) if e.not_found() => Ok(()),
        Err(e) => Err(e),
    }
}
