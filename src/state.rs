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
    blockchain_api::BlockchainApiProvider,
    build_info::BuildInfo,
    relay_client::http::Client,
    relay_rpc::{
        auth::ed25519_dalek::{SigningKey, VerifyingKey},
        domain::{DecodedClientId, DidKey},
        rpc::Receipt,
    },
    serde::{Deserialize, Serialize},
    sqlx::PgPool,
    std::{fmt, sync::Arc},
    tokio::sync::mpsc::Sender,
    tracing::info,
    wc::geoip::MaxMindResolver,
};

pub struct AppState {
    pub config: Configuration,
    pub analytics: NotifyAnalytics,
    pub build_info: BuildInfo,
    pub metrics: Option<Metrics>,
    pub postgres: PgPool,
    pub keypair: SigningKey,
    pub relay_client: Arc<Client>,
    pub relay_identity: DidKey,
    pub redis: Option<Arc<Redis>>,
    pub registry: Arc<Registry>,
    pub notify_keys: NotifyKeys,
    pub relay_mailbox_clearer_tx: Sender<Receipt>,
    pub clock: Clock,
    pub provider: Option<BlockchainApiProvider>,
    pub geoip_resolver: Option<Arc<MaxMindResolver>>,
}

build_info::build_info!(fn build_info);

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        analytics: NotifyAnalytics,
        config: Configuration,
        postgres: PgPool,
        keypair: SigningKey,
        keypair_seed: [u8; 32],
        relay_client: Arc<Client>,
        metrics: Option<Metrics>,
        redis: Option<Arc<Redis>>,
        registry: Arc<Registry>,
        relay_mailbox_clearer_tx: Sender<Receipt>,
        clock: Clock,
        provider: Option<BlockchainApiProvider>,
        geoip_resolver: Option<Arc<MaxMindResolver>>,
    ) -> Result<Self, NotifyServerError> {
        let build_info: &BuildInfo = build_info();

        let relay_identity = DidKey::from(DecodedClientId::from_key(
            &VerifyingKey::from_bytes(
                &hex::decode(&config.relay_public_key)
                    .unwrap()
                    .try_into()
                    .unwrap(),
            )
            .unwrap(),
        ));

        let notify_keys = NotifyKeys::new(&config.notify_url, keypair_seed)?;

        Ok(Self {
            analytics,
            config,
            build_info: build_info.clone(),
            metrics,
            postgres,
            keypair,
            relay_client,
            relay_identity,
            redis,
            registry,
            notify_keys,
            relay_mailbox_clearer_tx,
            clock,
            provider,
            geoip_resolver,
        })
    }

    pub async fn notify_webhook(
        &self,
        project_id: &str,
        event: WebhookNotificationEvent,
        account: &str,
    ) -> Result<(), NotifyServerError> {
        // if !is_valid_account(account) {
        //     info!("Didn't register account - invalid account: {account} for project {project_id}");
        //     return Err(NotifyServerError::InvalidAccount);
        // }

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
