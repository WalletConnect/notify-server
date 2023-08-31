use {
    crate::{networking, storage::redis::Addr as RedisAddr},
    serde::Deserialize,
    std::{net::IpAddr, str::FromStr},
};

#[derive(Deserialize, Debug, Clone, Eq, PartialEq)]
pub struct Configuration {
    #[serde(default = "public_ip")]
    pub public_ip: IpAddr,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    pub database_url: String,
    pub keypair_seed: String,
    pub project_id: String,
    pub relay_url: String,
    pub notify_url: String,

    pub registry_url: String,
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
}

impl Configuration {
    pub fn new() -> crate::Result<Configuration> {
        let config = envy::from_env::<Configuration>()?;
        Ok(config)
    }

    pub fn log_level(&self) -> tracing::Level {
        tracing::Level::from_str(self.log_level.as_str()).expect("Invalid log level")
    }

    pub fn auth_redis_addr(&self) -> Option<RedisAddr> {
        match (&self.auth_redis_addr_read, &self.auth_redis_addr_write) {
            (None, None) => None,
            (addr_read, addr_write) => Some(RedisAddr::from((addr_read, addr_write))),
        }
    }
}

fn default_port() -> u16 {
    3000
}

fn default_log_level() -> String {
    "DEBUG".to_string()
}

fn public_ip() -> IpAddr {
    networking::find_public_ip_addr().unwrap()
}
