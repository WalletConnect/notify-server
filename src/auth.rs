use {
    crate::error::Result,
    base64::Engine,
    chrono::{DateTime, Utc},
    relay_rpc::{
        auth::did::{DID_DELIMITER, DID_METHOD_KEY, DID_PREFIX},
        domain::DecodedClientId,
        jwt::JWT_HEADER_ALG,
    },
    serde::{Deserialize, Serialize},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionAuth {
    /// iat - timestamp when jwt was issued
    pub iat: u64,
    /// exp - timestamp when jwt must expire
    pub exp: u64,
    /// iss - did:key of an identity key. Enables to resolve attached blockchain
    /// account.4,
    pub iss: String,
    /// ksu - key server for identity key verification
    pub ksu: String,
    /// aud - did:key of an identity key. Enables to resolve associated Dapp
    /// domain used.
    pub aud: String,
    /// sub - blockchain account that notify subscription has been proposed for
    /// (did:pkh)
    pub sub: String,
    /// act - description of action intent. Must be equal to
    /// "notify_subscription"
    pub act: String,
    /// scp - scope of notification types authorized by the user
    pub scp: String,
    /// app - dapp's url
    pub app: String,
}

impl SubscriptionAuth {
    pub fn from_jwt(jwt: &str) -> Result<Self> {
        let mut parts = jwt.splitn(3, '.');
        let (Some(header), Some(claims)) = (parts.next(), parts.next()) else {
            return Err(AuthError::Format)?;
        };

        let header = base64::engine::general_purpose::STANDARD_NO_PAD.decode(header)?;
        let header = serde_json::from_slice::<JwtHeader>(&header)?;

        if header.alg != JWT_HEADER_ALG {
            return Err(AuthError::Algorithm)?;
        }

        let claims = base64::engine::general_purpose::STANDARD_NO_PAD.decode(claims)?;
        let claims = serde_json::from_slice::<SubscriptionAuth>(&claims)?;

        // TODO call verify_identity (and add keyserver to integration tests)

        if claims.exp < Utc::now().timestamp().unsigned_abs() {
            return Err(AuthError::Expired)?;
        }

        if claims.iat > Utc::now().timestamp_millis().unsigned_abs() {
            return Err(AuthError::NotYetValid)?;
        }

        if claims.act != "notify_subscription" {
            return Err(AuthError::InvalidAct)?;
        }

        let mut parts = jwt.rsplitn(2, '.');

        let (Some(signature), Some(message)) = (parts.next(), parts.next()) else {
            return Err(AuthError::Format)?;
        };

        let did_key = claims
            .iss
            .strip_prefix(DID_PREFIX)
            .ok_or(AuthError::IssuerPrefix)?
            .strip_prefix(DID_DELIMITER)
            .ok_or(AuthError::IssuerFormat)?
            .strip_prefix(DID_METHOD_KEY)
            .ok_or(AuthError::IssuerMethod)?
            .strip_prefix(DID_DELIMITER)
            .ok_or(AuthError::IssuerFormat)?;

        let pub_key = did_key.parse::<DecodedClientId>()?;

        let key = jsonwebtoken::DecodingKey::from_ed_der(pub_key.as_ref());

        // Finally, verify signature.
        let sig_result = jsonwebtoken::crypto::verify(
            signature,
            message.as_bytes(),
            &key,
            jsonwebtoken::Algorithm::EdDSA,
        );

        match sig_result {
            Ok(true) => Ok(claims),
            Ok(false) | Err(_) => Err(AuthError::InvalidSignature)?,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JwtHeader {
    typ: String,
    alg: String,
}

#[derive(thiserror::Error, Debug)]
pub enum AuthError {
    #[error("Invalid format")]
    Format,

    #[error("Invalid header")]
    InvalidHeader,

    #[error("Invalid issuer prefix")]
    IssuerPrefix,

    #[error("Invalid issuer format")]
    IssuerFormat,

    #[error("Invalid issuer method")]
    IssuerMethod,

    #[error("Invalid signature")]
    InvalidSignature,

    #[error("Invalid algorithm")]
    Algorithm,

    #[error("Failed to verify with keyserver")]
    CacaoValidation,

    #[error("Keyserver account mismatch")]
    CacaoAccountMismatch,

    #[error("Expired")]
    Expired,

    #[error("Not yet valid")]
    NotYetValid,

    #[error("Invalid act")]
    InvalidAct,
}

// TODO call this
pub async fn verify_identity(pubkey: &str, keyserver: &str, account: &str) -> Result<()> {
    let url = format!("{}/identity?publicKey={}", keyserver, pubkey);
    let res = reqwest::get(&url).await?;
    let cacao: KeyServerResponse = res.json().await?;

    if cacao.value.is_none() {
        return Err(AuthError::CacaoValidation)?;
    }

    let cacao = cacao.value.unwrap().cacao;

    // TODO verify `iss` signature
    if cacao.p.iss != account {
        return Err(AuthError::CacaoAccountMismatch)?;
    }

    if let Some(exp) = cacao.p.exp {
        let exp = DateTime::parse_from_rfc3339(&exp)?;

        if exp.timestamp() < Utc::now().timestamp() {
            return Err(AuthError::CacaoValidation)?;
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KeyServerResponse {
    status: String,
    error: Option<String>,
    value: Option<CacaoValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacaoValue {
    cacao: relay_rpc::auth::cacao::Cacao,
}
