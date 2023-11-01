use {
    crate::services::private_http::handlers::metrics::handler,
    axum::{routing::get, Router},
    std::net::SocketAddr,
    tracing::info,
};

mod handlers;

pub fn start(
    telemetry_prometheus_port: Option<u16>,
) -> hyper::Server<hyper::server::conn::AddrIncoming, axum::routing::IntoMakeService<axum::Router>>
{
    let private_port = telemetry_prometheus_port.unwrap_or(3001);
    let private_addr = SocketAddr::from(([0, 0, 0, 0], private_port));
    info!("Starting metric server on {}", private_addr);

    let private_app = Router::new().route("/metrics", get(handler));

    axum::Server::bind(&private_addr).serve(private_app.into_make_service())
}
