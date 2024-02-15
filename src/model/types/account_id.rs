use {
    once_cell::sync::OnceCell,
    relay_rpc::auth::did::{combine_did_data, extract_did_data, DidError},
    serde::{Deserialize, Serialize},
    sha2::Digest,
    sha3::Keccak256,
    std::sync::Arc,
};

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

#[derive(Debug, thiserror::Error)]
pub enum AccountIdError {
    #[error("Account ID is is not a valid CAIP-10 account ID or uses an unsupported namespace")]
    UnrecognizedChainId,

    #[error("Account ID is eip155 but does not pass ERC-55 checksum")]
    Eip155Erc55Fail,
}

impl TryFrom<String> for AccountId {
    type Error = AccountIdError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::try_from(s.as_ref())
    }
}

impl TryFrom<&str> for AccountId {
    type Error = AccountIdError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        if is_eip155_account(s) {
            if as_erc_55(s) != s {
                return Err(AccountIdError::Eip155Erc55Fail);
            }
            Ok(Self(Arc::from(s)))
        } else {
            Err(AccountIdError::UnrecognizedChainId)
        }
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

fn is_eip155_account(account_id: &str) -> bool {
    static PATTERN_CELL: OnceCell<regex::Regex> = OnceCell::new();
    let pattern =
        PATTERN_CELL.get_or_init(|| regex::Regex::new(r"^eip155:\d+:0x[0-9a-fA-F]{40}$").unwrap());
    pattern.is_match(account_id)
}

#[derive(Debug, thiserror::Error)]
pub enum AccountIdParseError {
    #[error(transparent)]
    Caip10Error(#[from] AccountIdError),

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

fn as_erc_55(s: &str) -> String {
    if s.starts_with("eip155:") {
        let ox = "0x";
        if let Some(ox_start) = s.find(ox) {
            let hex_start = ox_start + ox.len();
            s[0..hex_start]
                .chars()
                .chain(erc_55_checksum_encode(&s[hex_start..].to_ascii_lowercase()))
                .collect()
        } else {
            // If no 0x then address is very invalid anyway. Not validating this for now, goal is just to avoid duplicates.
            s.to_owned()
        }
    } else {
        s.to_owned()
    }
}

// Encodes a lowercase hex address with ERC-55 checksum
pub fn erc_55_checksum_encode(s: &str) -> impl Iterator<Item = char> + '_ {
    let address_hash = hex::encode(Keccak256::default().chain_update(s).finalize());
    s.chars().enumerate().map(move |(i, c)| {
        if !c.is_numeric() && address_hash.as_bytes()[i] > b'7' {
            c.to_ascii_uppercase()
        } else {
            c
        }
    })
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_eip155() {
        // https://github.com/ChainAgnostic/namespaces/blob/main/eip155/caip10.md#test-cases

        // Ethereum mainnet (valid/checksummed)
        let test = "eip155:1:0x22227A31dd842196A246d8f3b775998560eAa61d";
        assert_eq!(test, as_erc_55(test));

        // Ethereum mainnet (will not validate in EIP155-conformant systems)
        let test = "eip155:1:0x22227a31dd842196a246d8f3b775998560eaa61d";
        assert_ne!(test, as_erc_55(test));

        // Polygon mainnet (valid/checksummed)
        let test = "eip155:137:0x0495766cD136138Fc492Dd499B8DC87A92D6685b";
        assert_eq!(test, as_erc_55(test));

        // Polygon mainnet (will not validate in EIP155-conformant systems)
        let test = "eip155:137:0x0495766CD136138FC492DD499B8DC87A92D6685B";
        assert_ne!(test, as_erc_55(test));

        // Not EIP155
        let junk = "jkF53jF";
        assert_eq!(junk, as_erc_55(junk));
    }

    #[test]
    fn test_erc_55() {
        // https://eips.ethereum.org/EIPS/eip-55

        fn test(addr: &str) {
            let ox = "0x";
            assert_eq!(
                addr,
                ox.chars()
                    .chain(erc_55_checksum_encode(
                        &addr[ox.len()..].to_ascii_lowercase()
                    ))
                    .collect::<String>()
            );
        }

        test("0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed");
        test("0xfB6916095ca1df60bB79Ce92cE3Ea74c37c5d359");
        test("0xdbF03B407c01E7cD3CBea99509d93f8DDDC8C6FB");
        test("0xD1220A0cf47c7B9Be7A2E6BA89F429762e7b9aDb");
    }

    #[test]
    fn to_did_pkh() {
        let address = "eip155:1:0x1234567890123456789012345678901234567890";
        let account_id = AccountId::try_from(address).unwrap();
        assert_eq!(account_id.to_did_pkh(), format!("did:pkh:{address}"));
    }

    #[test]
    fn from_did_pkh() {
        let address = "eip155:1:0x1234567890123456789012345678901234567890";
        let account_id = AccountId::from_did_pkh(&format!("did:pkh:{address}")).unwrap();
        assert_eq!(account_id.as_ref(), address);
    }

    #[test]
    fn test_is_eip155_account() {
        assert!(is_eip155_account(
            "eip155:1:0x62639418051006514eD5Bb5B20aa7aAD642cC2d0"
        ));
        assert!(is_eip155_account(
            "eip155:2:0x62639418051006514eD5Bb5B20aa7aAD642cC2d0"
        ));
        assert!(is_eip155_account(
            "eip155:12:0x62639418051006514eD5Bb5B20aa7aAD642cC2d0"
        ));
        assert!(!is_eip155_account(
            "eip155:1:0x62639418051006514eD5Bb5B20aa7aAD642cC2d"
        ));
        assert!(!is_eip155_account(
            "eip156:1:0x62639418051006514eD5Bb5B20aa7aAD642cC2d0"
        ));
        assert!(!is_eip155_account(
            "eip15:1:0x62639418051006514eD5Bb5B20aa7aAD642cC2d0"
        ));
        assert!(!is_eip155_account(
            "0x62639418051006514eD5Bb5B20aa7aAD642cC2d0"
        ));
        assert!(!is_eip155_account(
            "eip155:12:62639418051006514eD5Bb5B20aa7aAD642cC2d0"
        ));
        assert!(!is_eip155_account(
            "62639418051006514eD5Bb5B20aa7aAD642cC2d0"
        ));
        assert!(!is_eip155_account(
            "eip155:1:0x62639418051006514eD5Bb5B20aa7aAD642cC2d00"
        ));
        assert!(!is_eip155_account(
            "eeip155:1:0x62639418051006514eD5Bb5B20aa7aAD642cC2d0"
        ));
    }

    #[test]
    fn requires_erc_55() {
        assert!(AccountId::try_from("eip155:1:0x62639418051006514eD5Bb5B20aa7aAD642cC2d0").is_ok());
        assert!(
            AccountId::try_from("eip155:1:0x62639418051006514eD5Bb5B20aa7aAD642cC2D0").is_err()
        );
    }
}
