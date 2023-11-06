use {
    crate::{error::Result, state::AppState},
    axum::{extract::State, http::StatusCode, response::IntoResponse, Json},
    data_encoding::BASE64URL,
    serde_json::json,
    std::sync::Arc,
    tracing::info,
};

pub async fn handler(State(state): State<Arc<AppState>>) -> Result<axum::response::Response> {
    info!("Serving did.json");

    let (did_id, key_agreement_key_id, authentication_key_id) = {
        let domain = &state.notify_keys.domain;
        let prefix = "did:web:";
        let did_id = format!("{prefix}{domain}");
        let key_agreement_key_id = format!("{prefix}{domain}#wc-notify-subscribe-key");
        let authentication_key_id = format!("{prefix}{domain}#wc-notify-authentication-key");
        (did_id, key_agreement_key_id, authentication_key_id)
    };

    let key_agreement = BASE64URL.encode(state.notify_keys.key_agreement_public.as_bytes());
    let authentication = BASE64URL.encode(state.notify_keys.authentication_public.as_bytes());

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

    Ok((StatusCode::OK, Json(json)).into_response())
}