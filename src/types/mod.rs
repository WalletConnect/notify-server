use {
    crate::{error::Result, state::WebhookNotificationEvent},
    chacha20poly1305::{aead::Aead, consts::U12, ChaCha20Poly1305, KeyInit},
    rand::{distributions::Uniform, prelude::Distribution, rngs::OsRng},
    serde::{Deserialize, Serialize},
    sha2::digest::generic_array::GenericArray,
    std::collections::HashSet,
    uuid::Uuid,
};

mod notification;

pub use notification::Notification;

// TODO move to Postgres
#[derive(Serialize, Deserialize, Debug)]
pub struct WebhookInfo {
    pub id: String,
    pub url: String,
    pub events: Vec<WebhookNotificationEvent>,
    pub project_id: String,
}

#[derive(Debug)]
pub struct Envelope<T> {
    pub envelope_type: u8,
    pub iv: [u8; 12],
    pub sealbox: Vec<u8>,
    pub opts: T,
}

impl Envelope<EnvelopeType0> {
    pub fn new(encryption_key: &[u8; 32], data: impl Serialize) -> Result<Self> {
        let serialized = serde_json::to_vec(&data)?;
        let iv = generate_nonce();

        let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(encryption_key));

        let sealbox = cipher
            .encrypt(&iv, &*serialized)
            .map_err(|_| crate::error::Error::EncryptionError("Encryption failed".into()))?;

        Ok(Self {
            envelope_type: 0,
            opts: EnvelopeType0 {},
            iv: iv.into(),
            sealbox,
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut serialized = vec![];
        serialized.push(self.envelope_type);
        serialized.extend_from_slice(&self.iv);
        serialized.extend_from_slice(&self.sealbox);
        serialized
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        Ok(Self {
            envelope_type: bytes[0],
            iv: bytes[1..13].try_into()?,
            sealbox: bytes[13..].to_vec(),
            opts: EnvelopeType0 {},
        })
    }
}

impl Envelope<EnvelopeType1> {
    pub fn new(
        encryption_key: &[u8; 32],
        data: serde_json::Value,
        pubkey: [u8; 32],
    ) -> Result<Self> {
        let serialized = serde_json::to_vec(&data)?;
        let iv = generate_nonce();

        let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(encryption_key));

        let sealbox = cipher
            .encrypt(&iv, &*serialized)
            .map_err(|_| crate::error::Error::EncryptionError("Encryption failed".into()))?;

        Ok(Self {
            envelope_type: 1,
            opts: EnvelopeType1 { pubkey },
            iv: iv.into(),
            sealbox,
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut serialized = vec![];
        serialized.push(self.envelope_type);
        serialized.extend_from_slice(&self.opts.pubkey);
        serialized.extend_from_slice(&self.iv);
        serialized.extend_from_slice(&self.sealbox);
        serialized
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        Ok(Self {
            envelope_type: bytes[0],
            opts: EnvelopeType1 {
                pubkey: bytes[1..33].try_into()?,
            },
            iv: bytes[33..45].try_into()?,
            sealbox: bytes[45..].to_vec(),
        })
    }

    pub fn pubkey(&self) -> [u8; 32] {
        self.opts.pubkey
    }
}

#[derive(Serialize)]
pub struct EnvelopeType0 {}

pub struct EnvelopeType1 {
    pub pubkey: [u8; 32],
}

fn generate_nonce() -> GenericArray<u8, U12> {
    let uniform = Uniform::from(0u8..=255);

    GenericArray::from_iter(uniform.sample_iter(&mut OsRng).take(12))
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Subscribtion {
    pub topic: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Unsubscribe {
    pub topic: String,
    pub id: String,
}

pub fn parse_scope(scope: &str) -> std::result::Result<HashSet<Uuid>, uuid::Error> {
    let types = scope
        .split(' ')
        .filter(|s| !s.is_empty())
        .map(Uuid::parse_str);
    let mut parsed_scope = HashSet::with_capacity(types.size_hint().0);
    for scope in types {
        let notification_type = scope?;
        parsed_scope.insert(notification_type);
    }
    Ok(parsed_scope)
}

pub fn encode_scope(notification_types: &HashSet<Uuid>) -> String {
    notification_types
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parse_empty_scope() {
        assert_eq!(parse_scope("").unwrap(), HashSet::new());
    }

    #[test]
    fn parse_one_scope() {
        let scope1 = Uuid::new_v4();
        assert_eq!(
            parse_scope(&format!("{scope1}")).unwrap(),
            HashSet::from([scope1])
        );
    }

    #[test]
    fn parse_two_scopes() {
        let scope1 = Uuid::new_v4();
        let scope2 = Uuid::new_v4();
        assert_eq!(
            parse_scope(&format!("{scope1} {scope2}")).unwrap(),
            HashSet::from([scope1, scope2])
        );
    }

    #[test]
    fn encode_empty_scope() {
        assert_eq!(encode_scope(&HashSet::new()), "");
    }

    #[test]
    fn encode_one_scope() {
        let scope1 = Uuid::new_v4();
        assert_eq!(encode_scope(&HashSet::from([scope1])), scope1.to_string());
    }

    #[test]
    fn encode_two_scopes() {
        let scope1 = Uuid::new_v4();
        let scope2 = Uuid::new_v4();
        let encoded = encode_scope(&HashSet::from([scope1, scope2]));
        // need to check both orders because HashSet is non-deterministic
        assert!(encoded == format!("{scope1} {scope2}") || encoded == format!("{scope2} {scope1}"));
    }
}
