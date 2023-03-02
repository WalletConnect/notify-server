use {self::server::CastServer, async_trait::async_trait, test_context::AsyncTestContext};

pub type ErrorResult<T> = Result<T, TestError>;

#[derive(Debug, thiserror::Error)]
pub enum TestError {
    #[error(transparent)]
    Elapsed(#[from] tokio::time::error::Elapsed),

    #[error(transparent)]
    CastServer(#[from] cast_server::error::Error),
}

mod server;

pub struct ServerContext {
    pub server: CastServer,
    pub project_id: String,
    pub relay_url: String,
}

#[async_trait]
impl AsyncTestContext for ServerContext {
    async fn setup() -> Self {
        let server = CastServer::start().await;

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
