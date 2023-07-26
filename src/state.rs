use {
    crate::{
        analytics::CastAnalytics,
        error::Result,
        metrics::Metrics,
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
}

build_info::build_info!(fn build_info);

impl AppState {
    pub fn new(
        analytics: CastAnalytics,
        config: Configuration,
        database: Arc<mongodb::Database>,
        keypair: Keypair,
        wsclient: Arc<relay_client::websocket::Client>,
        http_relay_client: Arc<relay_client::http::Client>,
        metrics: Option<Metrics>,
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
