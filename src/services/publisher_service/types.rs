use {
    sqlx::FromRow,
    std::{fmt, str::FromStr},
};

#[derive(Debug, PartialEq)]
pub enum SubscriberNotificationStatus {
    Queued,
    Processing,
    Published,
    NotSubscribed,
    WrongScope,
    RateLimited,
}

impl fmt::Display for SubscriberNotificationStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            SubscriberNotificationStatus::Queued => write!(f, "queued"),
            SubscriberNotificationStatus::Processing => write!(f, "processing"),
            SubscriberNotificationStatus::Published => write!(f, "published"),
            SubscriberNotificationStatus::NotSubscribed => write!(f, "not-subscribed"),
            SubscriberNotificationStatus::WrongScope => write!(f, "wrong-scope"),
            SubscriberNotificationStatus::RateLimited => write!(f, "rate-limited"),
        }
    }
}

impl FromStr for SubscriberNotificationStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "queued" => Ok(SubscriberNotificationStatus::Queued),
            "processing" => Ok(SubscriberNotificationStatus::Processing),
            "published" => Ok(SubscriberNotificationStatus::Published),
            "not-subscribed" => Ok(SubscriberNotificationStatus::NotSubscribed),
            "wrong-scope" => Ok(SubscriberNotificationStatus::WrongScope),
            "rate-limited" => Ok(SubscriberNotificationStatus::RateLimited),
            _ => Err(format!("'{}' is not a valid state", s)),
        }
    }
}

#[derive(Debug, FromRow)]
pub struct PublishingQueueStats {
    pub queued: i64,
    pub processing: i64,
}
