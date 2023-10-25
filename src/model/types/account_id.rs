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
        // If no 0x then address is very invalid anyway. Not validating this for now, goal is just to avoid duplicates.
        let hex_start = s.find("0x").map(|x| x + 2).unwrap_or(s.len());
        format!(
            "{}{}",
            &s[0..hex_start],
            erc_55_checksum_encode(&s[hex_start..].to_ascii_lowercase())
        )
    } else {
        s.to_owned()
    }
}

fn erc_55_checksum_encode(s: &str) -> String {
    let address_hash = hex::encode(
        Keccak256::default()
            .chain_update(s.to_ascii_lowercase())
            .finalize(),
    );
    s.chars()
        .enumerate()
        .map(|(i, c)| {
            if !c.is_numeric() && address_hash.as_bytes()[i] > b'7' {
                c.to_ascii_uppercase()
            } else {
                c
            }
        })
        .collect()
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_eip155() {
        // https://github.com/ChainAgnostic/namespaces/blob/main/eip155/caip10.md#test-cases

        // Ethereum mainnet (valid/checksummed)
        let ethereum_mainnet_valid = "eip155:1:0x22227A31dd842196A246d8f3b775998560eAa61d";
        assert_eq!(
            ethereum_mainnet_valid,
            ensure_erc_55(ethereum_mainnet_valid)
        );

        // Ethereum mainnet (will not validate in EIP155-conformant systems)
        let ethereum_mainnet_not_valid = "eip155:1:0x22227a31dd842196a246d8f3b775998560eaa61d";
        assert_ne!(
            ethereum_mainnet_not_valid,
            ensure_erc_55(ethereum_mainnet_not_valid)
        );

        // Polygon mainnet (valid/checksummed)
        let polygon_mainnet_valid = "eip155:137:0x0495766cD136138Fc492Dd499B8DC87A92D6685b";
        assert_eq!(polygon_mainnet_valid, ensure_erc_55(polygon_mainnet_valid));

        // Polygon mainnet (will not validate in EIP155-conformant systems)
        let polygon_mainnet_not_valid = "eip155:137:0x0495766CD136138FC492DD499B8DC87A92D6685B";
        assert_ne!(
            polygon_mainnet_not_valid,
            ensure_erc_55(polygon_mainnet_not_valid)
        );

        // Not EIP155
        let random = "jkF53jF";
        assert_eq!(random, ensure_erc_55(random));
    }

    #[test]
    fn test_erc_55() {
        // https://eips.ethereum.org/EIPS/eip-55

        fn test(addr: &str) {
            assert_eq!(addr, format!("0x{}", erc_55_checksum_encode(&addr[2..])));
        }

        test("0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed");
        test("0xfB6916095ca1df60bB79Ce92cE3Ea74c37c5d359");
        test("0xdbF03B407c01E7cD3CBea99509d93f8DDDC8C6FB");
        test("0xD1220A0cf47c7B9Be7A2E6BA89F429762e7b9aDb");
    }
}
