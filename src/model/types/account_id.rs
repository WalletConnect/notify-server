use {relay_rpc::new_type, sha2::Digest, sha3::Keccak256, std::sync::Arc};

new_type!(
    #[doc = "A CAIP-10 account ID which is guaranteed to be lowercase to avoid ERC-55 duplicates."]
    #[as_ref(forward)]
    AccountId: Arc<str>
);

impl From<String> for AccountId {
    fn from(s: String) -> Self {
        Self::from(s.as_ref())
    }
}

impl From<&str> for AccountId {
    fn from(s: &str) -> Self {
        Self(Arc::from(ensure_erc_55(s)))
    }
}

fn ensure_erc_55(s: &str) -> String {
    if s.starts_with("eip155:") {
        if let Some(zerox_start) = s.find("0x") {
            let hex_start = zerox_start + 2;
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
fn erc_55_checksum_encode(s: &str) -> impl Iterator<Item = char> + '_ {
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
        assert_eq!(test, ensure_erc_55(test));

        // Ethereum mainnet (will not validate in EIP155-conformant systems)
        let test = "eip155:1:0x22227a31dd842196a246d8f3b775998560eaa61d";
        assert_ne!(test, ensure_erc_55(test));

        // Polygon mainnet (valid/checksummed)
        let test = "eip155:137:0x0495766cD136138Fc492Dd499B8DC87A92D6685b";
        assert_eq!(test, ensure_erc_55(test));

        // Polygon mainnet (will not validate in EIP155-conformant systems)
        let test = "eip155:137:0x0495766CD136138FC492DD499B8DC87A92D6685B";
        assert_ne!(test, ensure_erc_55(test));

        // Not EIP155
        let junk = "jkF53jF";
        assert_eq!(junk, ensure_erc_55(junk));
    }

    #[test]
    fn test_erc_55() {
        // https://eips.ethereum.org/EIPS/eip-55

        fn test(addr: &str) {
            assert_eq!(
                addr,
                "0x".chars()
                    .chain(erc_55_checksum_encode(&addr[2..].to_ascii_lowercase()))
                    .collect::<String>()
            );
        }

        test("0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed");
        test("0xfB6916095ca1df60bB79Ce92cE3Ea74c37c5d359");
        test("0xdbF03B407c01E7cD3CBea99509d93f8DDDC8C6FB");
        test("0xD1220A0cf47c7B9Be7A2E6BA89F429762e7b9aDb");
    }
}
