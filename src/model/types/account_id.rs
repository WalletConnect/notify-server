use {relay_rpc::new_type, std::sync::Arc};

new_type!(
    #[doc = "A CAIP-10 account ID which is guaranteed to be lowercase to avoid ERC-55 duplicates."]
    #[as_ref(forward)]
    AccountId: Arc<str>
);

impl From<String> for AccountId {
    fn from(s: String) -> Self {
        Self(Arc::from(s.to_ascii_lowercase()))
    }
}

impl From<&str> for AccountId {
    fn from(s: &str) -> Self {
        Self(Arc::from(s.to_ascii_lowercase()))
    }
}
