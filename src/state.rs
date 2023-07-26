use {
    crate::{
        analytics::CastAnalytics,
        error::Result,
        metrics::Metrics,
        registry::Registry,
        types::{ClientData, LookupEntry, WebhookInfo},
        Configuration,
    },
    build_info::BuildInfo,
    futures::TryStreamExt,
    log::info,
    mongodb::{bson::doc, options::ReplaceOptions},
    relay_rpc::auth::ed25519_dalek::Keypair,
    serde::{Deserialize, Serialize},
    std::{fmt, sync::Arc},
    url::Url,
};

pub struct AppState {
    pub config: Configuration,
    pub analytics: CastAnalytics,
    pub build_info: BuildInfo,
    pub metrics: Option<Metrics>,
    pub database: Arc<mongodb::Database>,
    pub keypair: Keypair,
    pub wsclient: Arc<relay_client::websocket::Client>,
    pub http_relay_client: Arc<relay_client::http::Client>,
    pub registry: Arc<Registry>,
}

build_info::build_info!(fn build_info);

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        analytics: CastAnalytics,
        config: Configuration,
        database: Arc<mongodb::Database>,
        keypair: Keypair,
        wsclient: Arc<relay_client::websocket::Client>,
        http_relay_client: Arc<relay_client::http::Client>,
        metrics: Option<Metrics>,
        registry: Arc<Registry>,
    ) -> crate::Result<AppState> {
        let build_info: &BuildInfo = build_info();

        Ok(AppState {
            analytics,
            config,
            build_info: build_info.clone(),
            metrics,
            database,
            keypair,
            wsclient,
            http_relay_client,
            registry,
        })
    }

    pub async fn register_client(
        &self,
        project_id: &str,
        client_data: &ClientData,
        url: &Url,
    ) -> Result<()> {
        let key = hex::decode(client_data.sym_key.clone())?;
        let topic = sha256::digest(&*key);

        let insert_data = ClientData {
            id: client_data.id.clone(),
            relay_url: url.to_string().trim_end_matches('/').to_string(),
            sym_key: client_data.sym_key.clone(),
            scope: client_data.scope.clone(),
        };

        self.database
            .collection::<ClientData>(project_id)
            .replace_one(
                doc! { "_id": client_data.id.clone()},
                insert_data,
                ReplaceOptions::builder().upsert(true).build(),
            )
            .await?;

        self.database
            .collection::<LookupEntry>("lookup_table")
            .replace_one(
                doc! { "topic": &topic, "project_id": &project_id.to_string()},
                LookupEntry {
                    topic: topic.clone(),
                    project_id: project_id.to_string(),
                    account: client_data.id.clone(),
                },
                ReplaceOptions::builder().upsert(true).build(),
            )
            .await?;

        self.analytics
            .client(crate::analytics::client_info::ClientInfo {
                project_id: project_id.into(),
                account: client_data.id.clone().into(),
                topic: topic.clone().into(),
                registered_at: gorgon::time::now(),
            });

        self.wsclient.subscribe(topic.into()).await?;

        self.notify_webhook(
            project_id,
            WebhookNotificationEvent::Subscribed,
            &client_data.id,
        )
        .await?;

        Ok(())
    }

    pub async fn notify_webhook(
        &self,
        project_id: &str,
        event: WebhookNotificationEvent,
        account: &str,
    ) -> Result<()> {
        if !is_valid_account(account) {
            info!("Didn't register account - invalid account: {account} for project {project_id}");
            return Err(crate::error::Error::InvalidAccount);
        }

        info!(
            "Triggering webhook for project: {}, with account: {} and event \"{}\"",
            project_id, account, event
        );
        let mut cursor = self
            .database
            .collection::<WebhookInfo>("webhooks")
            .find(doc! { "project_id": project_id}, None)
            .await?;

        let client = reqwest::Client::new();

        // Interate over cursor
        while let Some(webhook) = cursor.try_next().await? {
            if !webhook.events.contains(&event) {
                continue;
            }

            let res = client
                .post(&webhook.url)
                .json(&WebhookMessage {
                    id: webhook.id.clone(),
                    event,
                    account: account.to_string(),
                })
                .send()
                .await?;

            info!(
                "Triggering webhook: {} resulted in http status: {}",
                webhook.id,
                res.status()
            );
        }

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
        assert_eq!(is_valid_account(ethereum_account), true);

        let cosmos_account =
            "cosmos:cosmoshub-2:\
             cosmospub1addwnpepqd5xvvdrw7dsfe89pcr9amlnvx9qdkjgznkm2rlfzesttpjp50jy2lueqp2";
        assert_eq!(is_valid_account(cosmos_account), true);

        let bitcoin_account =
            "bip122:000000000019d6689c085ae165831e93:1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa";
        assert_eq!(is_valid_account(bitcoin_account), true);
    }
}
