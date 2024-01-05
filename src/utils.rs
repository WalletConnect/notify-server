use relay_rpc::domain::Topic;

// TODO consider using the key object directly instead of a byte slice
pub fn topic_from_key(key: &[u8]) -> Topic {
    sha256::digest(key).into()
}
