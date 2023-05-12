use {
    self::server::CastServer,
    async_trait::async_trait,
    base64::Engine,
    cast_server::auth::SubscriptionAuth,
    ed25519_dalek::Signer,
    test_context::AsyncTestContext,
    walletconnect_sdk::rpc::auth::{
        ed25519_dalek::Keypair,
        JwtHeader,
        JWT_HEADER_ALG,
        JWT_HEADER_TYP,
    },
};

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

        let project_id = std::env::var("TEST_PROJECT_ID").unwrap();
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

pub fn encode_subscription_auth(subscription_auth: &SubscriptionAuth, keypair: &Keypair) -> String {
    let data = JwtHeader {
        typ: JWT_HEADER_TYP,
        alg: JWT_HEADER_ALG,
    };

    let header = serde_json::to_string(&data).unwrap();

    let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(header);

    let claims = {
        let json = serde_json::to_string(subscription_auth).unwrap();
        base64::engine::general_purpose::STANDARD_NO_PAD.encode(json)
    };

    let message = format!("{header}.{claims}");

    let signature =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(keypair.sign(message.as_bytes()));

    format!("{message}.{signature}")
}
