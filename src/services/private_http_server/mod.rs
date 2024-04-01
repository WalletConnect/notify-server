use {
    crate::services::private_http_server::handlers::metrics::handler, axum::{routing::get, Router}, std::net::{IpAddr, SocketAddr}, tokio::net::TcpListener, tracing::info
};

mod handlers;

pub async fn start(
    bind_ip: IpAddr,
    telemetry_prometheus_port: Option<u16>,
) -> Result<(), std::io::Error> {
    let private_app = Router::new().route("/metrics", get(handler));

    let port = telemetry_prometheus_port.unwrap_or(3001);
    let addr = SocketAddr::from((bind_ip, port));
    info!("Starting private HTTP server on {}", addr);

    axum::serve(TcpListener::bind(addr).await?, private_app.into_make_service())
        .await
}
