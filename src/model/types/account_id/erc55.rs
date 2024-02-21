use {sha2::Digest, sha3::Keccak256};

// https://eips.ethereum.org/EIPS/eip-55

// Encodes a lowercase hex address without '0x' with ERC-55 checksum
// If a non-lowercase hex value, or a non-address is passed, the behavior is undefined
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
mod tests {
    use super::*;

    #[test]
    fn test() {
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
        test("0x9AfEaC202C837df470b5A145e0EfD6a574B21029");
    }
}
