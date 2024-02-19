use {
    super::eip155::{validate_eip155, Eip155Error, NAMESPACE_EIP155},
    once_cell::sync::Lazy,
    regex::Regex,
    thiserror::Error,
};

#[derive(Debug, PartialEq, Eq, Error)]
pub enum Caip10Error {
    #[error("Account ID is is not a valid CAIP-10 account ID")]
    Invalid,

    #[error("Account ID uses an unsupported namespace")]
    UnsupportedNamespace,

    #[error("Account ID is eip155 namespace but: {0}")]
    Eip155(#[from] Eip155Error),
}

// https://github.com/ChainAgnostic/CAIPs/blob/main/CAIPs/caip-10.md#syntax
static PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^([-a-z0-9]{3,8}):([-_a-zA-Z0-9]{1,32}):([-.%a-zA-Z0-9]{1,128})$").unwrap()
});

pub fn validate_caip_10(s: &str) -> Result<(), Caip10Error> {
    if let Some(caps) = PATTERN.captures(s) {
        let (_, [namespace, reference, address]) = caps.extract();

        if namespace == NAMESPACE_EIP155 {
            validate_eip155(reference, address)?;
            Ok(())
        } else {
            Err(Caip10Error::UnsupportedNamespace)
        }
    } else {
        Err(Caip10Error::Invalid)
    }
}

#[cfg(test)]
mod tests {
    use {super::*, crate::model::types::AccountId};

    #[test]
    fn test() {
        assert!(AccountId::try_from("eip155:1:0x9AfEaC202C837df470b5A145e0EfD6a574B21029").is_ok());
        assert_eq!(AccountId::try_from("eip155:111111111111111111111111111111111:0x9AfEaC202C837df470b5A145e0EfD6a574B21029"), Err(Caip10Error::Invalid));
        assert_eq!(AccountId::try_from("junk"), Err(Caip10Error::Invalid));
    }

    #[test]
    fn account_id_valid_namespaces() {
        assert!(AccountId::try_from("eip155:1:0x9AfEaC202C837df470b5A145e0EfD6a574B21029").is_ok());
        assert_eq!(
            AccountId::try_from("junk:1:1"),
            Err(Caip10Error::UnsupportedNamespace)
        );
    }
}
