use {
    crate::{error::Result, state::WebhookNotificationEvent},
    chacha20poly1305::{aead::Aead, consts::U12, ChaCha20Poly1305, KeyInit},
    parquet::data_type::AsBytes,
    rand::{distributions::Uniform, prelude::Distribution},
    rand_core::OsRng,
    serde::{Deserialize, Serialize},
    sha2::digest::generic_array::GenericArray,
    std::collections::HashSet,
};

#[derive(Serialize, Deserialize, Debug)]
pub struct WebhookInfo {
    pub id: String,
    pub url: String,
    pub events: Vec<WebhookNotificationEvent>,
    pub project_id: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RegisterBody {
    pub account: String,
    pub relay_url: String,
    pub sym_key: String,
    pub subscription_auth: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClientData {
    #[serde(rename = "_id")]
    pub id: String,
    pub relay_url: String,
    pub sym_key: String,
    #[serde(default = "default_scope")]
    pub scope: HashSet<String>,
}

fn default_scope() -> HashSet<String> {
    let mut scope = HashSet::with_capacity(1);
    scope.insert("v1".into());
    scope
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LookupEntry {
    #[serde(rename = "_id")]
    pub topic: String,
    pub project_id: String,
    pub account: String,
}

#[derive(Debug)]
pub struct Envelope<T> {
    pub envelope_type: u8,
    pub iv: [u8; 12],
    pub sealbox: Vec<u8>,
    pub opts: T,
}

impl Envelope<EnvelopeType0> {
    pub fn new(encryption_key: &str, data: impl Serialize) -> Result<Self> {
        let serialized = serde_json::to_vec(&data)?;
        let iv = generate_nonce();
        let encryption_key = hex::decode(encryption_key)?;

        let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(&encryption_key));

        let sealbox = cipher
            .encrypt(&iv, serialized.as_bytes())
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
    pub fn new(encryption_key: &str, data: impl Serialize, pubkey: [u8; 32]) -> Result<Self> {
        let serialized = serde_json::to_vec(&data)?;
        let iv = generate_nonce();
        let encryption_key = hex::decode(encryption_key)?;

        let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(&encryption_key));

        let sealbox = cipher
            .encrypt(&iv, serialized.as_bytes())
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

    let mut rng = OsRng {};
    GenericArray::from_iter(uniform.sample_iter(&mut rng).take(12))
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

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct Notification {
    pub title: String,
    pub body: String,
    pub icon: String,
    pub url: String,
    #[serde(default = "default_notification_type")]
    pub r#type: String,
}

fn default_notification_type() -> String {
    "v1".to_string()
}
