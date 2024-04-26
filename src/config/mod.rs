use {
    crate::{
        error::NotifyServerError, rate_limit::Clock, registry::storage::redis::Addr as RedisAddr,
    },
    relay_rpc::domain::ProjectId,
    std::{env, net::IpAddr},
    url::Url,
};

mod deployed;
mod local;

#[derive(Debug, Clone)]
pub struct Configuration {
    pub public_ip: IpAddr,
    pub bind_ip: IpAddr,
    pub port: u16,
    pub log_level: String,
    pub postgres_url: String,
    pub postgres_max_connections: u32,
    pub keypair_seed: String,
    pub project_id: ProjectId,
    pub blockchain_api_endpoint: Option<String>,
    /// Relay URL e.g. https://relay.walletconnect.com
    pub relay_url: Url,
    pub relay_public_key: String,
    /// General external URL for where the Notify Server is listening on
    pub notify_url: Url,
    /// External URL for relay to deliver webhooks too
    pub webhook_notify_url: Url,

    pub registry_url: Url,
    pub registry_auth_token: String,

    pub auth_redis_addr_read: Option<String>,
    pub auth_redis_addr_write: Option<String>,
    pub redis_pool_size: u32,

    // TELEMETRY
    pub telemetry_prometheus_port: Option<u16>,

    // AWS
    pub s3_endpoint: Option<String>,

    // GeoIP
    pub geoip_db_bucket: Option<String>,
    pub geoip_db_key: Option<String>,

    // GeoBlocking
    pub blocked_countries: Vec<String>,

    // Analytics
    pub analytics_export_bucket: Option<String>,

    pub clock: Clock,
}

impl Configuration {
    pub fn auth_redis_addr(&self) -> Option<RedisAddr> {
        match (&self.auth_redis_addr_read, &self.auth_redis_addr_write) {
            (None, None) => None,
            (addr_read, addr_write) => Some(RedisAddr::from((addr_read, addr_write))),
        }
    }
}

pub async fn get_configuration() -> Result<Configuration, NotifyServerError> {
    if env::var("ENVIRONMENT") == Ok("DEPLOYED".to_owned()) {
        deployed::get_configuration()
    } else {
        local::get_configuration().await
    }
}
