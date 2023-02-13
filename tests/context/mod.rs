use {self::server::RustHttpStarter, async_trait::async_trait, test_context::AsyncTestContext};

mod server;

pub struct ServerContext {
    pub server: RustHttpStarter,
    pub project_id: String,
    pub relay_url: String,
}

#[async_trait]
impl AsyncTestContext for ServerContext {
    async fn setup() -> Self {
        let server = RustHttpStarter::start().await;

        let project_id = std::env::var("PROJECT_ID").unwrap();
        let relay_url = std::env::var("RELAY_URL").unwrap();

        Self {
            server,
            project_id,
            relay_url,
        }
    }

    async fn teardown(mut self) {
        self.server.shutdown().await;
    }
}
