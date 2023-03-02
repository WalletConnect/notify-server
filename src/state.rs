use {
    crate::{metrics::Metrics, Configuration},
    build_info::BuildInfo,
    ed25519_dalek::Keypair,
    std::sync::Arc,
};

pub struct AppState {
    pub config: Configuration,
    pub build_info: BuildInfo,
    pub metrics: Option<Metrics>,
    pub database: Arc<mongodb::Database>,
    pub keypair: Keypair,
}

build_info::build_info!(fn build_info);

impl AppState {
    pub fn new(
        config: Configuration,
        database: Arc<mongodb::Database>,
        keypair: Keypair,
    ) -> crate::Result<AppState> {
        let build_info: &BuildInfo = build_info();

        Ok(AppState {
            config,
            build_info: build_info.clone(),
            metrics: None,
            database,
            keypair,
        })
    }

    pub fn set_metrics(&mut self, metrics: Metrics) {
        self.metrics = Some(metrics);
    }
}
