use {
    dotenv::dotenv,
    notify_server::{config::Configuration, Result},
    tracing_subscriber::fmt::format::FmtSpan,
};

/// Service for processing queued messages for publish
#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    let config = Configuration::new().expect("Failed to load config!");
    tracing_subscriber::fmt()
        .with_env_filter(config.clone().log_level)
        .with_span_events(FmtSpan::CLOSE)
        .with_ansi(std::env::var("ANSI_LOGS").is_ok())
        .init();
    notify_server::services::publisher_service::bootstrap(config).await
}
