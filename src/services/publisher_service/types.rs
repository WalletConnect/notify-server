use {
    ed25519_dalek::SigningKey,
    relay_rpc::domain::ClientId,
    serde::{Deserialize, Serialize},
    sqlx::FromRow,
    std::{fmt, str::FromStr, sync::Arc},
    uuid::Uuid,
};

#[derive(Debug)]
pub struct ProjectSigningDetails {
    pub identity: ClientId,
    pub private_key: SigningKey,
    pub app: Arc<str>,
}

#[derive(Debug, FromRow)]
pub struct ProcessingMessageItem {
    pub id: String,
    pub notification: String,
    pub subscriber: String,
    pub project: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct JwtMessage {
    pub iat: i64, // issued at
    pub exp: i64, // expiry
    // TODO: This was changed from notify pubkey, should be confirmed if we want to keep this
    pub iss: String,       // dapps identity key
    pub act: String,       // action intent (must be "notify_message")
    pub sub: String,       // did:pkh of blockchain account
    pub app: String,       // dapp domain url
    pub msg: Notification, // message
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Debug, FromRow)]
pub struct Subscriber {
    pub id: Uuid,
    pub inserted_at: String,
    pub updated_at: String,
    pub project: Uuid,
    pub acount: String,
    pub sym_key: String,
    pub topic: String,
    pub expiry: String,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Debug, FromRow)]
pub struct Notification {
    pub id: Uuid,
    pub project: String,
    pub created_at: String,
    pub r#type: String,
    pub title: String,
    pub body: String,
    pub icon: String,
    pub url: String,
}

#[derive(Debug, PartialEq)]
pub enum NotificationStates {
    Queued,
    Processing,
    Published,
    NotSubscribed,
    WrongScope,
    RateLimited,
}

impl fmt::Display for NotificationStates {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            NotificationStates::Queued => write!(f, "queued"),
            NotificationStates::Processing => write!(f, "processing"),
            NotificationStates::Published => write!(f, "published"),
            NotificationStates::NotSubscribed => write!(f, "not-subscribed"),
            NotificationStates::WrongScope => write!(f, "wrong-scope"),
            NotificationStates::RateLimited => write!(f, "rate-limited"),
        }
    }
}

impl FromStr for NotificationStates {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "queued" => Ok(NotificationStates::Queued),
            "processing" => Ok(NotificationStates::Processing),
            "published" => Ok(NotificationStates::Published),
            "not-subscribed" => Ok(NotificationStates::NotSubscribed),
            "wrong-scope" => Ok(NotificationStates::WrongScope),
            "rate-limited" => Ok(NotificationStates::RateLimited),
            _ => Err(format!("'{}' is not a valid state", s)),
        }
    }
}
