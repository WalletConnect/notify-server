use {
    crate::state::WebhookNotificationEvent,
    serde::{Deserialize, Serialize},
};

// FIXME
// pub mod delete_webhook;
// pub mod get_webhooks;
// pub mod register_webhook;
// pub mod update_webhook;

#[derive(Debug, Deserialize, Serialize)]
pub struct WebhookConfig {
    #[serde(rename = "webhook")]
    url: String,
    events: Vec<WebhookNotificationEvent>,
}

// fn validate_url(url: &str) -> Result<()> {
//     let url = url::Url::parse(url)?;
//     if url.scheme() != "https" {
//         return Err(crate::error::Error::InvalidScheme);
//     }
//     Ok(())
// }
