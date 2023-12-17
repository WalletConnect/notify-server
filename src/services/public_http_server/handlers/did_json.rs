use {
    crate::{auth::DidWeb, error::Result, notify_keys::NotifyKeys, state::AppState},
    axum::{extract::State, http::StatusCode, response::IntoResponse, Json},
    data_encoding::BASE64URL,
    serde::{Deserialize, Serialize},
    std::sync::Arc,
    tracing::info,
};

// No rate limit necessary since returning a fixed string is less computational intensive than tracking the rate limit

// TODO generate this response at app startup to avoid unnecessary string allocations
pub async fn handler(State(state): State<Arc<AppState>>) -> Result<axum::response::Response> {
    info!("Serving did.json");

    let json = generate_json(&state.notify_keys);

    Ok((StatusCode::OK, Json(json)).into_response())
}

pub const WC_NOTIFY_SUBSCRIBE_KEY_ID: &str = "#wc-notify-subscribe-key";
pub const WC_NOTIFY_AUTHENTICATION_KEY_ID: &str = "#wc-notify-authentication-key";

const CONTEXT_DID_V1: &str = "https://www.w3.org/ns/did/v1";
const CONTEXT_JWS_V1: &str = "https://w3id.org/security/suites/jws-2020/v1";
const JSON_WEB_KEY_2020: &str = "JsonWebKey2020";
const OKP: &str = "OKP";
const X25519: &str = "X25519";
const ED25519: &str = "Ed25519";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidJson {
    #[serde(rename = "@context")]
    pub context: Vec<String>,
    pub id: DidWeb,
    pub verification_method: Vec<VerificationMethod>,
    pub key_agreement: Vec<String>,
    pub authentication: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationMethod {
    pub id: String,
    pub r#type: String,
    pub controller: DidWeb,
    pub public_key_jwk: PublicKeyJwk,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PublicKeyJwk {
    pub kty: String,
    pub crv: String,
    pub x: String,
}

fn generate_json(notify_keys: &NotifyKeys) -> DidJson {
    let did_id = DidWeb::from_domain(notify_keys.domain.clone());
    let key_agreement_key_id = format!("{did_id}{WC_NOTIFY_SUBSCRIBE_KEY_ID}");
    let authentication_key_id = format!("{did_id}{WC_NOTIFY_AUTHENTICATION_KEY_ID}");

    let key_agreement = BASE64URL.encode(notify_keys.key_agreement_public.as_bytes());
    let authentication = BASE64URL.encode(notify_keys.authentication_public.as_bytes());

    DidJson {
        context: vec![CONTEXT_DID_V1.to_string(), CONTEXT_JWS_V1.to_string()],
        id: did_id.clone(),
        verification_method: vec![
            VerificationMethod {
                id: key_agreement_key_id.clone(),
                r#type: JSON_WEB_KEY_2020.to_owned(),
                controller: did_id.clone(),
                public_key_jwk: PublicKeyJwk {
                    kty: OKP.to_string(),
                    crv: X25519.to_string(),
                    x: key_agreement,
                },
            },
            VerificationMethod {
                id: authentication_key_id.clone(),
                r#type: JSON_WEB_KEY_2020.to_owned(),
                controller: did_id,
                public_key_jwk: PublicKeyJwk {
                    kty: OKP.to_string(),
                    crv: ED25519.to_string(),
                    x: authentication,
                },
            },
        ],
        key_agreement: vec![key_agreement_key_id],
        authentication: vec![authentication_key_id],
    }
}

#[cfg(test)]
mod test {
    use {super::*, serde_json::json, url::Url};

    #[test]
    fn test_json_encoding() {
        let notify_keys = NotifyKeys::new(
            &Url::parse("https://notify.example.com").unwrap(),
            rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng()),
        )
        .unwrap();

        let (did_id, key_agreement_key_id, authentication_key_id) = {
            let domain = &notify_keys.domain;
            let prefix = "did:web:";
            let did_id = format!("{prefix}{domain}");
            let key_agreement_key_id = format!("{prefix}{domain}#wc-notify-subscribe-key");
            let authentication_key_id = format!("{prefix}{domain}#wc-notify-authentication-key");
            (did_id, key_agreement_key_id, authentication_key_id)
        };

        let key_agreement = BASE64URL.encode(notify_keys.key_agreement_public.as_bytes());
        let authentication = BASE64URL.encode(notify_keys.authentication_public.as_bytes());

        let json = json!({
          "@context": [
            "https://www.w3.org/ns/did/v1",
            "https://w3id.org/security/suites/jws-2020/v1"
          ],
          "id": did_id,
          "verificationMethod": [
            {
              "id": key_agreement_key_id,
              "type": "JsonWebKey2020",
              "controller": did_id,
              "publicKeyJwk": {
                "kty": "OKP",
                "crv": "X25519",
                "x": key_agreement
              }
            },
            {
              "id": authentication_key_id,
              "type": "JsonWebKey2020",
              "controller": did_id,
              "publicKeyJwk": {
                "kty": "OKP",
                "crv": "Ed25519",
                "x": authentication
              }
            },
          ],
          "keyAgreement": [
            key_agreement_key_id
          ],
          "authentication": [
            authentication_key_id
          ],
        });

        assert_eq!(
            serde_json::to_value(generate_json(&notify_keys)).unwrap(),
            json,
        );
    }
}
