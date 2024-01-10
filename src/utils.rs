use relay_rpc::domain::{DecodedClientId, Topic};

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
