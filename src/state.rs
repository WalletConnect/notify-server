use {
    crate::{
        analytics::NotifyAnalytics,
        error::NotifyServerError,
        metrics::Metrics,
        notify_keys::NotifyKeys,
        rate_limit::Clock,
        registry::{storage::redis::Redis, Registry},
        Configuration,
    },
    build_info::BuildInfo,
    relay_rpc::auth::{
        cacao::signature::eip1271::blockchain_api::BlockchainApiProvider, ed25519_dalek::Keypair,
    },
    serde::{Deserialize, Serialize},
    sqlx::PgPool,
    std::{fmt, sync::Arc},
    tracing::info,
};

pub struct AppState {
    pub config: Configuration,
    pub analytics: NotifyAnalytics,
    pub build_info: BuildInfo,
    pub metrics: Option<Metrics>,
    pub postgres: PgPool,
    pub keypair: Keypair,
    pub relay_ws_client: Arc<relay_client::websocket::Client>,
    pub relay_http_client: Arc<relay_client::http::Client>,
    pub redis: Option<Arc<Redis>>,
    pub registry: Arc<Registry>,
    pub notify_keys: NotifyKeys,
    pub clock: Clock,
    pub provider: BlockchainApiProvider,
}

build_info::build_info!(fn build_info);

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        analytics: NotifyAnalytics,
        config: Configuration,
        postgres: PgPool,
        keypair: Keypair,
        keypair_seed: [u8; 32],
        relay_ws_client: Arc<relay_client::websocket::Client>,
        relay_http_client: Arc<relay_client::http::Client>,
        metrics: Option<Metrics>,
        redis: Option<Arc<Redis>>,
        registry: Arc<Registry>,
        clock: Clock,
        provider: BlockchainApiProvider,
    ) -> Result<Self, NotifyServerError> {
        let build_info: &BuildInfo = build_info();

        let notify_keys = NotifyKeys::new(&config.notify_url, keypair_seed)?;

        Ok(Self {
            analytics,
            config,
            build_info: build_info.clone(),
            metrics,
            postgres,
            keypair,
            relay_ws_client,
            relay_http_client,
            redis,
            registry,
            notify_keys,
            clock,
            provider,
        })
    }

    pub async fn notify_webhook(
        &self,
        project_id: &str,
        event: WebhookNotificationEvent,
        account: &str,
    ) -> Result<(), NotifyServerError> {
        if !is_valid_account(account) {
            info!("Didn't register account - invalid account: {account} for project {project_id}");
            return Err(NotifyServerError::InvalidAccount);
        }

        info!(
            "Triggering webhook for project: {}, with account: {} and event \"{}\"",
            project_id, account, event
        );
        // TODO
        // let mut cursor = self
        //     .database
        //     .collection::<WebhookInfo>("webhooks")
        //     .find(doc! { "project_id": project_id}, None)
        //     .await?;

        // let client = reqwest::Client::new();

        // // Interate over cursor
        // while let Some(webhook) = cursor.try_next().await? {
        //     if !webhook.events.contains(&event) {
        //         continue;
        //     }

        //     let res = client
        //         .post(&webhook.url)
        //         .json(&WebhookMessage {
        //             id: webhook.id.clone(),
        //             event,
        //             account: account.to_string(),
        //         })
        //         .send()
        //         .await?;

        //     info!(
        //         "Triggering webhook: {} resulted in http status: {}",
        //         webhook.id,
        //         res.status()
        //     );
        // }

        Ok(())
    }
}

lazy_static! {
     // chain_id:    namespace + ":" + reference
    // namespace:   [-a-z0-9]{3,8}
    // reference:   [-_a-zA-Z0-9]{1,32}
    // account_id:  chain_id + ":" + address
    // address:     any chain address
    // Unwrap is ok as this is a static regex
    static ref VALID_ACCOUNT_REGEX: regex::Regex  = regex::Regex::new(r"^[-a-z0-9]{3,8}:[-_a-zA-Z0-9]{1,32}:.{1,100}$").unwrap();
}

fn is_valid_account(account: &str) -> bool {
    VALID_ACCOUNT_REGEX.is_match(account)
}

#[derive(Serialize, Deserialize, Debug)]
pub struct WebhookMessage {
    pub id: String,
    pub event: WebhookNotificationEvent,
    pub account: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum WebhookNotificationEvent {
    Subscribed,
    Unsubscribed,
}

impl fmt::Display for WebhookNotificationEvent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            WebhookNotificationEvent::Subscribed => write!(f, "subscribed"),
            WebhookNotificationEvent::Unsubscribed => write!(f, "unsubscribed"),
        }
    }
}

#[cfg(test)]
mod test {
    use crate::state::is_valid_account;

    #[test]
    fn test_regex() {
        let ethereum_account = "eip155:1:0x5ccbc5dbb84097463acb6b0382f0254ed6c1cb62";
        assert!(is_valid_account(ethereum_account));

        let cosmos_account = "cosmos:cosmoshub-2:\
             cosmospub1addwnpepqd5xvvdrw7dsfe89pcr9amlnvx9qdkjgznkm2rlfzesttpjp50jy2lueqp2";
        assert!(is_valid_account(cosmos_account));

        let bitcoin_account =
            "bip122:000000000019d6689c085ae165831e93:1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa";
        assert!(is_valid_account(bitcoin_account));
    }
}
