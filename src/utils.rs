use {
    crate::model::types::AccountId,
    relay_rpc::domain::{DecodedClientId, Topic},
};

// TODO consider using the key object directly instead of a byte slice
pub fn topic_from_key(key: &[u8]) -> Topic {
    sha256::digest(key).into()
}

pub fn get_client_id(verifying_key: &ed25519_dalek::VerifyingKey) -> DecodedClientId {
    // Better approach, but dependency versions conflict right now.
    // See: https://github.com/WalletConnect/WalletConnectRust/issues/53
    // DecodedClientId::from_key(verifying_key)
    DecodedClientId(verifying_key.to_bytes())
}

pub fn get_address_from_account(account: &AccountId) -> &str {
    let s = account.as_ref();
    let known_skippable_prefix_len = "eip155:1".len();
    let i = s[known_skippable_prefix_len..]
        .find(':')
        .expect("AccountId should have already been validated to be eip155");
    &s[known_skippable_prefix_len + i + 1..]
}

pub fn is_same_address(account1: &AccountId, account2: &AccountId) -> bool {
    get_address_from_account(account1).eq_ignore_ascii_case(get_address_from_account(account2))
}
