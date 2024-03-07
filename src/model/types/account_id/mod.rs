use {
    self::caip10::{validate_caip_10, Caip10Error},
    relay_rpc::auth::did::{combine_did_data, extract_did_data, DidError},
    serde::{Deserialize, Serialize},
    std::sync::Arc,
};

pub mod caip10;
pub mod eip155;
pub mod erc55;

#[derive(
    Debug,
    Hash,
    Clone,
    PartialEq,
    Eq,
    ::derive_more::Display,
    ::derive_more::From,
    ::derive_more::AsRef,
)]
#[doc = "A CAIP-10 account ID."]
#[as_ref(forward)]
pub struct AccountId(Arc<str>);

impl AccountId {
    pub fn value(&self) -> &Arc<str> {
        &self.0
    }

    pub fn into_value(self) -> Arc<str> {
        self.0
    }
}

impl TryFrom<String> for AccountId {
    type Error = Caip10Error;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::try_from(s.as_ref())
    }
}

impl TryFrom<&str> for AccountId {
    type Error = Caip10Error;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        validate_caip_10(s)?;
        Ok(Self(Arc::from(s)))
    }
}

impl Serialize for AccountId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_ref())
    }
}

impl<'a> Deserialize<'a> for AccountId {
    fn deserialize<D: serde::Deserializer<'a>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::try_from(s).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AccountIdParseError {
    #[error(transparent)]
    Caip10Error(#[from] Caip10Error),

    #[error("DID error: {0}")]
    Did(#[from] DidError),
}

const DID_METHOD_PKH: &str = "pkh";

impl AccountId {
    pub fn from_did_pkh(did: &str) -> Result<Self, AccountIdParseError> {
        extract_did_data(did, DID_METHOD_PKH)
            .map_err(AccountIdParseError::Did)?
            .try_into()
            .map_err(AccountIdParseError::Caip10Error)
    }

    pub fn to_did_pkh(&self) -> String {
        combine_did_data(DID_METHOD_PKH, self.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_did_pkh() {
        let address = "eip155:1:0x9AfEaC202C837df470b5A145e0EfD6a574B21029";
        let account_id = AccountId::try_from(address).unwrap();
        assert_eq!(account_id.to_did_pkh(), format!("did:pkh:{address}"));
    }

    #[test]
    fn from_did_pkh() {
        let address = "eip155:1:0x9AfEaC202C837df470b5A145e0EfD6a574B21029";
        let account_id = AccountId::from_did_pkh(&format!("did:pkh:{address}")).unwrap();
        assert_eq!(account_id.as_ref(), address);
    }
}
