use {
    crate::{metrics::Metrics, unregister_service::UnregisterMessage, Configuration},
    build_info::BuildInfo,
    std::sync::Arc,
    walletconnect_sdk::rpc::auth::ed25519_dalek::Keypair,
};

pub struct AppState {
    pub config: Configuration,
    pub build_info: BuildInfo,
    pub metrics: Option<Metrics>,
    pub database: Arc<mongodb::Database>,
    pub keypair: Keypair,
    pub unregister_tx: tokio::sync::mpsc::Sender<UnregisterMessage>,
}

build_info::build_info!(fn build_info);

impl AppState {
    pub fn new(
        config: Configuration,
        database: Arc<mongodb::Database>,
        keypair: Keypair,
        unregister_tx: tokio::sync::mpsc::Sender<UnregisterMessage>,
    ) -> crate::Result<AppState> {
        let build_info: &BuildInfo = build_info();

        Ok(AppState {
            config,
            build_info: build_info.clone(),
            metrics: None,
            database,
            keypair,
            unregister_tx,
        })
    }

    pub fn set_metrics(&mut self, metrics: Metrics) {
        self.metrics = Some(metrics);
    }
}
