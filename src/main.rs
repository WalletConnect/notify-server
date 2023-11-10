use {
    dotenv::dotenv,
    notify_server::{config::Configuration, error::Result},
    tokio::sync::broadcast,
    tracing_subscriber::fmt::format::FmtSpan,
};

#[tokio::main]
async fn main() -> Result<()> {
    load_dot_env()?;
    let config = Configuration::new()?;

    tracing_subscriber::fmt()
        .with_env_filter(&config.log_level)
        .with_span_events(FmtSpan::CLOSE)
        .with_ansi(std::env::var("ANSI_LOGS").is_ok())
        .init();

    let (_signal, shutdown) = broadcast::channel(1);
    notify_server::bootstrap(shutdown, config).await
}

fn load_dot_env() -> dotenv::Result<()> {
    match dotenv() {
        Ok(_) => Ok(()),
        Err(e) if e.not_found() => Ok(()),
        Err(e) => Err(e),
    }
}
