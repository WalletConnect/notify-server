use {
    cast_server::{config::Configuration, Result},
    dotenv::dotenv,
    tokio::sync::broadcast,
};

#[tokio::main]
async fn main() -> Result<()> {
    // let logger = log::Logger::init().expect("Failed to start logging");

    let (_signal, shutdown) = broadcast::channel(1);
    dotenv().ok();

    let config = Configuration::new().expect("Failed to load config!");
    let result = cast_server::bootstap(shutdown, config).await;

    // logger.stop();
    result
}
