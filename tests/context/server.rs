use {
    super::ErrorResult,
    cast_server::config::Configuration,
    std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener},
    tokio::{
        runtime::Handle,
        sync::broadcast,
        time::{sleep, Duration},
    },
};

pub struct CastServer {
    pub public_addr: SocketAddr,
    private_port: u16,
    shutdown_signal: tokio::sync::broadcast::Sender<()>,
    is_shutdown: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {}

impl CastServer {
    pub async fn start() -> Self {
        let public_port = get_random_port();
        let mut private_port = get_random_port();
        while public_port == private_port {
            private_port = get_random_port();
        }
        let rt = Handle::current();
        let public_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), public_port);

        let (signal, shutdown) = broadcast::channel(1);

        let project_id = std::env::var("PROJECT_ID").unwrap();
        let relay_url = std::env::var("RELAY_URL").unwrap();
        let cast_url = std::env::var("CAST_URL").unwrap();
        let test_keypair_seed = std::env::var("TEST_KEYPAIR_SEED").unwrap();

        std::thread::spawn(move || {
            rt.block_on(async move {
                let config: Configuration = Configuration {
                    port: public_port,
                    log_level: "INFO".into(),
                    public_ip: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                    database_url: "mongodb://localhost:27017".into(),
                    project_id,
                    keypair_seed: test_keypair_seed,
                    is_test: true,
                    otel_exporter_otlp_endpoint: None,
                    telemetry_prometheus_port: Some(private_port),
                    relay_url: relay_url.replace("http", "ws"),
                    cast_url,
                    analytics_s3_endpoint: None,
                    analytics_export_bucket: "".to_string(),
                    analytics_geoip_db_bucket: None,
                    analytics_geoip_db_key: None,
                };

                cast_server::bootstap(shutdown, config).await
            })
            .unwrap();
        });

        if let Err(e) = wait_for_server_to_start(public_port, private_port).await {
            panic!("Failed to start server with error: {e:?}")
        }

        Self {
            public_addr,
            shutdown_signal: signal,
            private_port,
            is_shutdown: false,
        }
    }

    pub async fn shutdown(&mut self) {
        if self.is_shutdown {
            return;
        }
        self.is_shutdown = true;
        let _ = self.shutdown_signal.send(());

        wait_for_server_to_shutdown(self.public_addr.port(), self.private_port)
            .await
            .unwrap();
    }
}

// Finds a free port.
fn get_random_port() -> u16 {
    use std::sync::atomic::{AtomicU16, Ordering};

    static NEXT_PORT: AtomicU16 = AtomicU16::new(9000);

    loop {
        let port = NEXT_PORT.fetch_add(1, Ordering::SeqCst);

        if is_port_available(port) {
            return port;
        }
    }
}

fn is_port_available(port: u16) -> bool {
    TcpListener::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port)).is_ok()
}

async fn wait_for_server_to_shutdown(port: u16, private_port: u16) -> ErrorResult<()> {
    let poll_fut = async {
        while !is_port_available(port) && !is_port_available(private_port) {
            sleep(Duration::from_millis(10)).await;
        }
    };

    Ok(tokio::time::timeout(Duration::from_secs(3), poll_fut).await?)
}

async fn wait_for_server_to_start(port: u16, private_port: u16) -> ErrorResult<()> {
    let poll_fut = async {
        while is_port_available(port) || is_port_available(private_port) {
            sleep(Duration::from_millis(10)).await;
        }
    };

    Ok(tokio::time::timeout(Duration::from_secs(5), poll_fut).await?)
}
