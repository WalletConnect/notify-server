use {
    crate::{
        error::{Error, Result},
        utils::{get_client_id, topic_from_key},
    },
    rand_chacha::{
        rand_core::{RngCore, SeedableRng},
        ChaCha20Rng,
    },
    relay_rpc::domain::{DecodedClientId, Topic},
    url::Url,
};

pub struct NotifyKeys {
    pub domain: String,
    pub key_agreement_secret: x25519_dalek::StaticSecret,
    pub key_agreement_public: x25519_dalek::PublicKey,
    pub key_agreement_topic: Topic,
    pub authentication_secret: ed25519_dalek::SigningKey,
    pub authentication_public: ed25519_dalek::VerifyingKey,
    pub authentication_client_id: DecodedClientId,
}

impl NotifyKeys {
    pub fn new(notify_url: &Url, keypair_seed: [u8; 32]) -> Result<Self> {
        let domain = notify_url
            .host_str()
            .ok_or(Error::UrlMissingHost)?
            .to_owned();

        // Use specific RNG instead of StdRng because StdRng can change implementations
        // between releases
        let get_rng = || ChaCha20Rng::from_seed(keypair_seed);

        let key_agreement_secret = x25519_dalek::StaticSecret::from({
            let mut key_agreement_secret: [u8; 32] = [0; 32];
            get_rng().fill_bytes(&mut key_agreement_secret);
            key_agreement_secret
        });
        let key_agreement_public = x25519_dalek::PublicKey::from(&key_agreement_secret);

        let authentication_secret = ed25519_dalek::SigningKey::generate(&mut get_rng());
        let authentication_public = ed25519_dalek::VerifyingKey::from(&authentication_secret);
        let authentication_client_id = get_client_id(&authentication_public);

        Ok(Self {
            domain,
            key_agreement_secret,
            key_agreement_public,
            key_agreement_topic: topic_from_key(key_agreement_public.as_bytes()),
            authentication_secret,
            authentication_public,
            authentication_client_id,
        })
    }
}
