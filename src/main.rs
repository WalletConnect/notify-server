use {
    notify_server::{bootstrap, config::get_configuration, error::Result},
    tokio::sync::broadcast,
    tracing_subscriber::fmt::format::FmtSpan,
};

#[tokio::main]
async fn main() -> Result<()> {
    let config = get_configuration()?;

    tracing_subscriber::fmt()
        .with_env_filter(&config.log_level)
        .with_span_events(FmtSpan::CLOSE)
        .with_ansi(std::env::var("ANSI_LOGS").is_ok())
        .init();

    let (_signal, shutdown) = broadcast::channel(1);
    bootstrap(shutdown, config).await
}
