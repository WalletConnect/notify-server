use {
    crate::model::types::erc55::erc_55_checksum_encode, once_cell::sync::Lazy, regex::Regex,
    thiserror::Error,
};

// https://github.com/ChainAgnostic/namespaces/blob/main/eip155/caip10.md
pub const NAMESPACE_EIP155: &str = "eip155";

// https://github.com/ChainAgnostic/namespaces/blob/main/eip155/caip2.md#syntax
static REFERENCE_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\d+$").expect("Safe unwrap: panics should be caught by test cases"));

static ADDRESS_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^0x([0-9a-fA-F]{40})$")
        .expect("Safe unwrap: panics should be caught by test cases")
});

#[derive(Debug, PartialEq, Eq, Error)]
pub enum Eip155Error {
    #[error("reference does not validate: {0}")]
    Reference(#[from] Eip155ReferenceError),

    #[error("address does not validate: {0}")]
    Address(#[from] Eip155AddressError),
}

pub fn validate_eip155(reference: &str, address: &str) -> Result<(), Eip155Error> {
    validate_eip155_reference(reference)?;
    validate_eip155_address(address)?;
    Ok(())
}

#[derive(Debug, PartialEq, Eq, Error)]
pub enum Eip155ReferenceError {
    #[error("does not match regex")]
    Regex,
}

fn validate_eip155_reference(reference: &str) -> Result<(), Eip155ReferenceError> {
    if REFERENCE_PATTERN.is_match(reference) {
        Ok(())
    } else {
        Err(Eip155ReferenceError::Regex)
    }
}

#[derive(Debug, PartialEq, Eq, Error)]
pub enum Eip155AddressError {
    #[error("does not match regex")]
    Regex,

    #[error("does not pass ERC-55 checksum")]
    Erc55,
}

fn validate_eip155_address(address: &str) -> Result<(), Eip155AddressError> {
    if let Some(caps) = ADDRESS_PATTERN.captures(address) {
        let (_, [address_hex]) = caps.extract();
        let erc55: String =
            erc_55_checksum_encode(&address_hex.to_ascii_lowercase()).collect::<String>();
        if erc55 == address_hex {
            Ok(())
        } else {
            Err(Eip155AddressError::Erc55)
        }
    } else {
        Err(Eip155AddressError::Regex)
    }
}

pub mod test_utils {
    use {
        super::erc_55_checksum_encode, crate::model::types::AccountId, k256::ecdsa::SigningKey,
        rand::rngs::OsRng, sha2::Digest, sha3::Keccak256,
    };

    pub fn generate_eoa() -> (SigningKey, String) {
        let account_signing_key = SigningKey::random(&mut OsRng);
        let address = &Keccak256::default()
            .chain_update(
                &account_signing_key
                    .verifying_key()
                    .to_encoded_point(false)
                    .as_bytes()[1..],
            )
            .finalize()[12..];
        let address = format!(
            "0x{}",
            erc_55_checksum_encode(&hex::encode(address)).collect::<String>()
        );
        (account_signing_key, address)
    }

    pub fn format_eip155_account(chain_id: u32, address: &str) -> AccountId {
        AccountId::try_from(format!("eip155:{chain_id}:{address}")).unwrap()
    }

    pub fn generate_account() -> (SigningKey, AccountId) {
        let (account_signing_key, address) = generate_eoa();
        let account = format_eip155_account(1, &address);
        (account_signing_key, account)
    }
}

#[cfg(test)]
pub mod tests {
    use {super::*, crate::model::types::account_id::caip10::validate_caip_10};

    #[test]
    fn test_eip155() {
        // https://github.com/ChainAgnostic/namespaces/blob/main/eip155/caip10.md#test-cases

        // Ethereum mainnet (valid/checksummed)
        let test = "eip155:1:0x22227A31dd842196A246d8f3b775998560eAa61d";
        assert!(validate_caip_10(test).is_ok());

        // Ethereum mainnet (will not validate in EIP155-conformant systems)
        let test = "eip155:1:0x22227a31dd842196a246d8f3b775998560eaa61d";
        assert!(validate_caip_10(test).is_err());

        // Polygon mainnet (valid/checksummed)
        let test = "eip155:137:0x0495766cD136138Fc492Dd499B8DC87A92D6685b";
        assert!(validate_caip_10(test).is_ok());

        // Polygon mainnet (will not validate in EIP155-conformant systems)
        let test = "eip155:137:0x0495766CD136138FC492DD499B8DC87A92D6685B";
        assert!(validate_caip_10(test).is_err());
    }

    #[test]
    fn validate_fn_uses_address_check() {
        let address = "123";
        let reference = "1";
        assert_eq!(
            validate_eip155(reference, address),
            Err(Eip155Error::Address(Eip155AddressError::Regex))
        );
    }

    #[test]
    fn validate_fn_uses_reference_check() {
        let address = "0x62639418051006514eD5Bb5B20aa7aAD642cC2d0";
        let reference = "abc";
        assert_eq!(
            validate_eip155(reference, address),
            Err(Eip155Error::Reference(Eip155ReferenceError::Regex))
        );
    }

    #[test]
    fn test_is_valid_eip155_account() {
        assert!(validate_caip_10("eip155:1:0x62639418051006514eD5Bb5B20aa7aAD642cC2d0").is_ok());
        assert!(validate_caip_10("eip155:2:0x62639418051006514eD5Bb5B20aa7aAD642cC2d0").is_ok());
        assert!(validate_caip_10("eip155:12:0x62639418051006514eD5Bb5B20aa7aAD642cC2d0").is_ok());
        assert!(validate_caip_10("eip155:1:0x62639418051006514eD5Bb5B20aa7aAD642cC2d").is_err());
        assert!(validate_caip_10("eip156:1:0x62639418051006514eD5Bb5B20aa7aAD642cC2d0").is_err());
        assert!(validate_caip_10("eip15:1:0x62639418051006514eD5Bb5B20aa7aAD642cC2d0").is_err());
        assert!(validate_caip_10("0x62639418051006514eD5Bb5B20aa7aAD642cC2d0").is_err());
        assert!(validate_caip_10("eip155:12:62639418051006514eD5Bb5B20aa7aAD642cC2d0").is_err());
        assert!(validate_caip_10("62639418051006514eD5Bb5B20aa7aAD642cC2d0").is_err());
        assert!(validate_caip_10("eip155:1:0x62639418051006514eD5Bb5B20aa7aAD642cC2d00").is_err());
        assert!(validate_caip_10("eeip155:1:0x62639418051006514eD5Bb5B20aa7aAD642cC2d0").is_err());
    }

    #[test]
    fn reference() {
        assert_eq!(
            validate_eip155_reference("f"),
            Err(Eip155ReferenceError::Regex)
        );
    }

    #[test]
    fn address_regex_fail() {
        assert_eq!(
            validate_eip155_address("0x62639418051006514eD5Bb5B20aa7aAD642cC2d"),
            Err(Eip155AddressError::Regex)
        );
        assert_eq!(
            validate_eip155_address("0x62639418051006514eD5Bb5B20aa7aAD642cC2d00"),
            Err(Eip155AddressError::Regex)
        );
    }

    #[test]
    fn requires_erc_55() {
        assert!(validate_eip155_address("0x62639418051006514eD5Bb5B20aa7aAD642cC2d0").is_ok());
        assert_eq!(
            validate_eip155_address("0x62639418051006514eD5Bb5B20aa7aAD642cC2D0"),
            Err(Eip155AddressError::Erc55)
        );
    }
}
