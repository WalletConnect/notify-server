use {
    super::Configuration,
    crate::error::NotifyServerError,
    relay_rpc::domain::ProjectId,
    serde::Deserialize,
    std::net::{IpAddr, Ipv4Addr},
    url::Url,
};

mod networking;

// Configuration entrypoint for a deployed (Terraform) service

#[derive(Deserialize, Debug)]
pub struct DeployedConfiguration {
    #[serde(default = "public_ip")]
    pub public_ip: IpAddr,
    #[serde(default = "default_bind_ip")]
    pub bind_ip: IpAddr,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    pub postgres_url: String,
    pub postgres_max_connections: u32,
    pub keypair_seed: String,
    pub project_id: ProjectId,
    /// Websocket URL e.g. wss://relay.walletconnect.com
    pub relay_url: Url,
    pub notify_url: Url,

    pub registry_url: Url,
    pub registry_auth_token: String,

    pub auth_redis_addr_read: Option<String>,
    pub auth_redis_addr_write: Option<String>,
    #[serde(default = "default_redis_pool_size")]
    pub redis_pool_size: u32,

    // TELEMETRY
    pub telemetry_prometheus_port: Option<u16>,

    // AWS
    pub s3_endpoint: Option<String>,

    // GeoIP
    pub geoip_db_bucket: Option<String>,
    pub geoip_db_key: Option<String>,

    // GeoBlocking
    #[serde(default)]
    pub blocked_countries: Vec<String>,

    // Analytics
    pub analytics_export_bucket: Option<String>,
}

fn public_ip() -> IpAddr {
    networking::find_public_ip_addr().unwrap()
}

fn default_bind_ip() -> IpAddr {
    IpAddr::V4(Ipv4Addr::UNSPECIFIED)
}

fn default_port() -> u16 {
    3000
}

fn default_log_level() -> String {
    "WARN,notify_server=INFO".to_string()
}

fn default_redis_pool_size() -> u32 {
    64
}

pub fn get_configuration() -> Result<Configuration, NotifyServerError> {
    let config = envy::from_env::<DeployedConfiguration>()?;

    Ok(Configuration {
        public_ip: config.public_ip,
        bind_ip: config.bind_ip,
        port: config.port,
        log_level: config.log_level,
        postgres_url: config.postgres_url,
        postgres_max_connections: config.postgres_max_connections,
        keypair_seed: config.keypair_seed,
        project_id: config.project_id,
        relay_url: config.relay_url,
        notify_url: config.notify_url,
        registry_url: config.registry_url,
        registry_auth_token: config.registry_auth_token,
        auth_redis_addr_read: config.auth_redis_addr_read,
        auth_redis_addr_write: config.auth_redis_addr_write,
        redis_pool_size: config.redis_pool_size,
        telemetry_prometheus_port: config.telemetry_prometheus_port,
        s3_endpoint: config.s3_endpoint,
        geoip_db_bucket: config.geoip_db_bucket,
        geoip_db_key: config.geoip_db_key,
        blocked_countries: config.blocked_countries,
        analytics_export_bucket: config.analytics_export_bucket,
        clock: None,
    })
}
