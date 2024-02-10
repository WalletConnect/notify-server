use std::time::Duration;

// Methods
// https://specs.walletconnect.com/2.0/specs/clients/notify/rpc-methods
// TODO switch to Lazy<Arc<str>>
pub const NOTIFY_WATCH_SUBSCRIPTIONS_METHOD: &str = "wc_notifyWatchSubscriptions";
pub const NOTIFY_SUBSCRIPTIONS_CHANGED_METHOD: &str = "wc_notifySubscriptionsChanged";
pub const NOTIFY_SUBSCRIBE_METHOD: &str = "wc_notifySubscribe";
pub const NOTIFY_MESSAGE_METHOD: &str = "wc_notifyMessage";
pub const NOTIFY_UPDATE_METHOD: &str = "wc_notifyUpdate";
pub const NOTIFY_DELETE_METHOD: &str = "wc_notifyDelete";
pub const NOTIFY_GET_NOTIFICATIONS_METHOD: &str = "wc_notifyGetNotifications";

// Tags
// https://specs.walletconnect.com/2.0/specs/clients/notify/rpc-methods
pub const NOTIFY_SUBSCRIBE_TAG: u32 = 4000;
pub const NOTIFY_SUBSCRIBE_RESPONSE_TAG: u32 = 4001;
pub const NOTIFY_MESSAGE_TAG: u32 = 4002;
pub const NOTIFY_MESSAGE_RESPONSE_TAG: u32 = 4003;
pub const NOTIFY_DELETE_TAG: u32 = 4004;
pub const NOTIFY_DELETE_RESPONSE_TAG: u32 = 4005;
pub const NOTIFY_UPDATE_TAG: u32 = 4008;
pub const NOTIFY_UPDATE_RESPONSE_TAG: u32 = 4009;
pub const NOTIFY_WATCH_SUBSCRIPTIONS_TAG: u32 = 4010;
pub const NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_TAG: u32 = 4011;
pub const NOTIFY_SUBSCRIPTIONS_CHANGED_TAG: u32 = 4012;
pub const NOTIFY_SUBSCRIPTIONS_CHANGED_RESPONSE_TAG: u32 = 4013;
pub const NOTIFY_NOOP_TAG: u32 = 4050;
pub const NOTIFY_GET_NOTIFICATIONS_TAG: u32 = 4014;
pub const NOTIFY_GET_NOTIFICATIONS_RESPONSE_TAG: u32 = 4015;

pub const INCOMING_TAGS: [u32; 7] = [
    NOTIFY_SUBSCRIBE_TAG,
    NOTIFY_MESSAGE_RESPONSE_TAG,
    NOTIFY_DELETE_TAG,
    NOTIFY_UPDATE_TAG,
    NOTIFY_WATCH_SUBSCRIPTIONS_TAG,
    NOTIFY_SUBSCRIPTIONS_CHANGED_RESPONSE_TAG,
    NOTIFY_GET_NOTIFICATIONS_TAG,
];

// TTLs
// https://specs.walletconnect.com/2.0/specs/clients/notify/rpc-methods
// https://specs.walletconnect.com/2.0/specs/clients/notify/notify-authentication
const T300: Duration = Duration::from_secs(300); // 5 min
const T2592000: Duration = Duration::from_secs(2592000); // 30 days
pub const NOTIFY_SUBSCRIBE_TTL: Duration = T300;
pub const NOTIFY_SUBSCRIBE_RESPONSE_TTL: Duration = T2592000;
pub const NOTIFY_MESSAGE_TTL: Duration = T2592000;
pub const NOTIFY_MESSAGE_RESPONSE_TTL: Duration = T2592000;
pub const NOTIFY_DELETE_TTL: Duration = T2592000;
pub const NOTIFY_DELETE_RESPONSE_TTL: Duration = T2592000;
pub const NOTIFY_UPDATE_TTL: Duration = T300;
pub const NOTIFY_UPDATE_RESPONSE_TTL: Duration = T2592000;
pub const NOTIFY_WATCH_SUBSCRIPTIONS_TTL: Duration = T300;
pub const NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_TTL: Duration = T300;
pub const NOTIFY_SUBSCRIPTIONS_CHANGED_TTL: Duration = T300;
pub const NOTIFY_SUBSCRIPTIONS_CHANGED_RESPONSE_TTL: Duration = T300;
pub const NOTIFY_NOOP_TTL: Duration = T300;
pub const NOTIFY_GET_NOTIFICATIONS_TTL: Duration = T300;
pub const NOTIFY_GET_NOTIFICATIONS_RESPONSE_TTL: Duration = T300;

// acts
// https://specs.walletconnect.com/2.0/specs/clients/notify/notify-authentication
// TODO switch to Lazy<Arc<str>>
pub const NOTIFY_MESSAGE_ACT: &str = "notify_message";
pub const NOTIFY_MESSAGE_RESPONSE_ACT: &str = "notify_message_response";
pub const NOTIFY_WATCH_SUBSCRIPTIONS_ACT: &str = "notify_watch_subscriptions";
pub const NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_ACT: &str = "notify_watch_subscriptions_response";
pub const NOTIFY_SUBSCRIPTIONS_CHANGED_ACT: &str = "notify_subscriptions_changed";
pub const NOTIFY_SUBSCRIPTIONS_CHANGED_RESPONE_ACT: &str = "notify_subscriptions_changed_response";
pub const NOTIFY_SUBSCRIBE_ACT: &str = "notify_subscription";
pub const NOTIFY_SUBSCRIBE_RESPONSE_ACT: &str = "notify_subscription_response";
pub const NOTIFY_UPDATE_ACT: &str = "notify_update";
pub const NOTIFY_UPDATE_RESPONSE_ACT: &str = "notify_update_response";
pub const NOTIFY_DELETE_ACT: &str = "notify_delete";
pub const NOTIFY_DELETE_RESPONSE_ACT: &str = "notify_delete_response";
pub const NOTIFY_GET_NOTIFICATIONS_ACT: &str = "notify_get_notifications";
pub const NOTIFY_GET_NOTIFICATIONS_RESPONSE_ACT: &str = "notify_get_notifications_response";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t2592000_is_30_days() {
        assert_eq!(T2592000.as_secs(), 30 * 24 * 60 * 60);
    }
}
