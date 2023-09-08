use {
    crate::{
        error::{Error, Result},
        websocket_service::decode_key,
    },
    rand_chacha::{
        rand_core::{RngCore, SeedableRng},
        ChaCha20Rng,
    },
    relay_rpc::domain::Topic,
    url::Url,
};

pub struct NotifyKeys {
    pub domain: String,
    pub key_agreement_secret: x25519_dalek::StaticSecret,
    pub key_agreement_public: x25519_dalek::PublicKey,
    pub key_agreement_topic: Topic,
    pub authentication_secret: ed25519_dalek::SigningKey,
    pub authentication_public: ed25519_dalek::VerifyingKey,
}

impl NotifyKeys {
    pub fn new(notify_url: &str, keypair_seed: &str) -> Result<Self> {
        let domain = Url::parse(notify_url)?
            .host_str()
            .ok_or(Error::UrlMissingHost)?
            .to_owned();

        let seed = decode_key(&sha256::digest(keypair_seed.as_bytes()))?;

        // Use specific RNG instead of StdRng because StdRng can change implementations
        // between releases
        let get_rng = || ChaCha20Rng::from_seed(seed);

        let key_agreement_secret = x25519_dalek::StaticSecret::from({
            let mut key_agreement_secret: [u8; 32] = [0; 32];
            get_rng().fill_bytes(&mut key_agreement_secret);
            key_agreement_secret
        });
        let key_agreement_public = x25519_dalek::PublicKey::from(&key_agreement_secret);

        let authentication_secret = ed25519_dalek::SigningKey::generate(&mut get_rng());
        let authentication_public = ed25519_dalek::VerifyingKey::from(&authentication_secret);

        Ok(Self {
            domain,
            key_agreement_secret,
            key_agreement_public,
            key_agreement_topic: Topic::from(sha256::digest(key_agreement_public.as_bytes())),
            authentication_secret,
            authentication_public,
        })
    }
}
