use {
    super::{IdentityKeyDetails, RelayClient},
    chacha20poly1305::{aead::Aead, ChaCha20Poly1305, KeyInit},
    chrono::{DateTime, Utc},
    data_encoding::BASE64,
    notify_server::{
        auth::{add_ttl, from_jwt, GetSharedClaims, SharedClaims},
        rpc::{derive_key, NotifyResponse, ResponseAuth},
        types::{Envelope, EnvelopeType0, EnvelopeType1},
        utils::topic_from_key,
    },
    relay_rpc::{domain::DecodedClientId, rpc::SubscriptionData},
    serde::de::DeserializeOwned,
    sha2::digest::generic_array::GenericArray,
};

pub fn decode_message<T>(msg: SubscriptionData, key: &[u8; 32]) -> T
where
    T: DeserializeOwned,
{
    let Envelope::<EnvelopeType0> { sealbox, iv, .. } =
        Envelope::<EnvelopeType0>::from_bytes(BASE64.decode(msg.message.as_bytes()).unwrap())
            .unwrap();
    let decrypted_response = ChaCha20Poly1305::new(GenericArray::from_slice(key))
        .decrypt(&iv.into(), chacha20poly1305::aead::Payload::from(&*sealbox))
        .unwrap();
    serde_json::from_slice::<T>(&decrypted_response).unwrap()
}

pub fn decode_response_message<T>(msg: SubscriptionData, key: &[u8; 32]) -> (u64, T)
where
    T: GetSharedClaims + DeserializeOwned,
{
    let response = decode_message::<NotifyResponse<ResponseAuth>>(msg, key);
    (
        response.id,
        from_jwt::<T>(&response.result.response_auth).unwrap(),
    )
}

pub struct TopicEncryptionSchemeAsymetric {
    pub client_private: x25519_dalek::StaticSecret,
    pub client_public: x25519_dalek::PublicKey,
    pub server_public: x25519_dalek::PublicKey,
}

pub enum TopicEncrptionScheme {
    Asymetric(TopicEncryptionSchemeAsymetric),
    Symetric([u8; 32]),
}

#[allow(clippy::too_many_arguments)]
pub async fn publish_jwt_message(
    relay_client: &mut RelayClient,
    client_id: &DecodedClientId,
    identity_key_details: &IdentityKeyDetails,
    encryption_details: &TopicEncrptionScheme,
    tag: u32,
    ttl: std::time::Duration,
    act: &str,
    mjv: Option<String>,
    make_message: impl FnOnce(SharedClaims) -> serde_json::Value,
) {
    fn make_shared_claims(
        now: DateTime<Utc>,
        ttl: std::time::Duration,
        act: &str,
        client_id: &DecodedClientId,
        mjv: Option<String>,
        identity_key_details: &IdentityKeyDetails,
    ) -> SharedClaims {
        SharedClaims {
            iat: now.timestamp() as u64,
            exp: add_ttl(now, ttl).timestamp() as u64,
            iss: identity_key_details.client_id.to_did_key(),
            act: act.to_owned(),
            aud: client_id.to_did_key(),
            mjv: mjv.unwrap_or_else(|| "0".to_owned()),
        }
    }

    let now = Utc::now();

    let message = make_message(make_shared_claims(
        now,
        ttl,
        act,
        client_id,
        mjv,
        identity_key_details,
    ));

    let (envelope, topic) = match encryption_details {
        TopicEncrptionScheme::Asymetric(TopicEncryptionSchemeAsymetric {
            client_private: client_secret,
            client_public,
            server_public,
        }) => {
            let response_topic_key = derive_key(server_public, client_secret).unwrap();
            (
                Envelope::<EnvelopeType1>::new(
                    &response_topic_key,
                    message,
                    *client_public.as_bytes(),
                )
                .unwrap()
                .to_bytes(),
                topic_from_key(server_public.as_bytes()),
            )
        }
        TopicEncrptionScheme::Symetric(sym_key) => (
            Envelope::<EnvelopeType0>::new(sym_key, message)
                .unwrap()
                .to_bytes(),
            topic_from_key(sym_key),
        ),
    };

    let message = BASE64.encode(&envelope);

    relay_client.publish(topic, message, tag, ttl).await;
}
