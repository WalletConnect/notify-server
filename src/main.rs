use {
    cast_server::{config::Configuration, Result},
    dotenv::dotenv,
    tokio::sync::broadcast,
};

#[tokio::main]
async fn main() -> Result<()> {
    let (_signal, shutdown) = broadcast::channel(1);
    dotenv().ok();

    let config = Configuration::new().expect("Failed to load config!");
    cast_server::bootstap(shutdown, config).await
}
