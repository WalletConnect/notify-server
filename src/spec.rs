use std::time::Duration;

// Tags
// https://specs.walletconnect.com/2.0/specs/clients/notify/rpc-methods
pub const NOTIFY_SUBSCRIBE_TAG: u32 = 4000;
pub const NOTIFY_SUBSCRIBE_RESPONSE_TAG: u32 = 4001;
pub const NOTIFY_MESSAGE_TAG: u32 = 4002;
pub const NOTIFY_DELETE_TAG: u32 = 4004;
pub const NOTIFY_DELETE_RESPONSE_TAG: u32 = 4005;
pub const NOTIFY_UPDATE_TAG: u32 = 4008;
pub const NOTIFY_UPDATE_RESPONSE_TAG: u32 = 4009;
pub const NOTIFY_WATCH_SUBSCRIPTIONS_TAG: u32 = 4010;
pub const NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_TAG: u32 = 4011;
pub const NOTIFY_WATCH_SUBSCRIPTIONS_CHANGED_TAG: u32 = 4012;

// TTLs
// https://specs.walletconnect.com/2.0/specs/clients/notify/rpc-methods
// https://specs.walletconnect.com/2.0/specs/clients/notify/notify-authentication
const T30: Duration = Duration::from_secs(30);
const T300: Duration = Duration::from_secs(300); // 5 min
const T2592000: Duration = Duration::from_secs(2592000); // 30 days
pub const NOTIFY_SUBSCRIBE_TTL: Duration = T30;
pub const NOTIFY_SUBSCRIBE_RESPONSE_TTL: Duration = T2592000;
pub const NOTIFY_MESSAGE_TTL: Duration = T2592000;
pub const NOTIFY_MESSAGE_RESPONSE_TTL: Duration = T2592000;
pub const NOTIFY_DELETE_TTL: Duration = T2592000;
pub const NOTIFY_DELETE_RESPONSE_TTL: Duration = T2592000;
pub const NOTIFY_UPDATE_TTL: Duration = T30;
pub const NOTIFY_UPDATE_RESPONSE_TTL: Duration = T2592000;
pub const NOTIFY_WATCH_SUBSCRIPTIONS_TTL: Duration = T300;
pub const NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_TTL: Duration = T300;
pub const NOTIFY_WATCH_SUBSCRIPTIONS_CHANGED_TTL: Duration = T300;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t2592000_is_30_days() {
        assert_eq!(T2592000.as_secs(), 30 * 24 * 60 * 60);
    }
}
