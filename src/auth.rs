use {
    crate::error::Result,
    base64::Engine,
    chrono::{DateTime, Duration as CDuration, Utc},
    ed25519_dalek::Signer,
    hyper::StatusCode,
    relay_rpc::{
        auth::{
            cacao::CacaoError,
            did::{DID_DELIMITER, DID_METHOD_KEY, DID_PREFIX},
        },
        domain::DecodedClientId,
        jwt::{JwtHeader, JWT_HEADER_ALG, JWT_HEADER_TYP},
    },
    reqwest::Response,
    serde::{de::DeserializeOwned, Deserialize, Serialize},
    serde_json::Value,
    std::{collections::HashSet, time::Duration},
    url::Url,
};

pub const STATEMENT: &str = "I further authorize this app to send and receive messages on my behalf using my WalletConnect identity. Read more at https://walletconnect.com/identity";
pub const STATEMENT_ALL_DOMAINS: &str = "I further authorize this app to send and receive messages on my behalf for ALL domains using my WalletConnect identity. Read more at https://walletconnect.com/identity";
pub const STATEMENT_THIS_DOMAIN: &str = "I further authorize this app to send and receive messages on my behalf for THIS domain using my WalletConnect identity. Read more at https://walletconnect.com/identity";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedClaims {
    /// timestamp when jwt was issued
    pub iat: u64,
    /// timestamp when jwt must expire
    pub exp: u64,
    /// did:key of client identity key or dapp or Notify Server
    /// authentication key
    pub iss: String,
    /// did:key of client identity key or dapp or Notify Server
    /// authentication key
    pub aud: String,
    /// description of action intent
    pub act: String,
}

pub fn add_ttl(now: DateTime<Utc>, ttl: Duration) -> DateTime<Utc> {
    let ttl = CDuration::from_std(ttl).expect("TTL out of range");
    now + ttl
}

pub trait GetSharedClaims {
    fn get_shared_claims(&self) -> &SharedClaims;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchSubscriptionsRequestAuth {
    #[serde(flatten)]
    pub shared_claims: SharedClaims,
    /// ksu - key server for identity key verification
    pub ksu: String,
    /// did:pkh
    pub sub: String,
    /// did:web of app domain to watch, or `null` for all domains
    #[serde(default)]
    pub app: Option<String>,
}

impl GetSharedClaims for WatchSubscriptionsRequestAuth {
    fn get_shared_claims(&self) -> &SharedClaims {
        &self.shared_claims
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchSubscriptionsResponseAuth {
    #[serde(flatten)]
    pub shared_claims: SharedClaims,
    /// did:pkh
    pub sub: String,
    /// array of Notify Server Subscriptions
    pub sbs: Vec<NotifyServerSubscription>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotifyServerSubscription {
    /// dApp url that the subscription refers to
    pub app_domain: String,
    /// Symetric key used for notify topic. sha256 to get notify topic to manage
    /// the subscription and call wc_notifySubscriptionUpdate and
    /// wc_notifySubscriptionDelete
    pub sym_key: String,
    /// CAIP-10 account
    pub account: String,
    /// Array of notification types enabled for this subscription
    pub scope: HashSet<String>,
    /// Unix timestamp of expiration
    pub expiry: u64,
}

impl GetSharedClaims for WatchSubscriptionsResponseAuth {
    fn get_shared_claims(&self) -> &SharedClaims {
        &self.shared_claims
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchSubscriptionsChangedRequestAuth {
    #[serde(flatten)]
    pub shared_claims: SharedClaims,
    /// did:pkh
    pub sub: String,
    /// array of Notify Server Subscriptions
    pub sbs: Vec<NotifyServerSubscription>,
}

impl GetSharedClaims for WatchSubscriptionsChangedRequestAuth {
    fn get_shared_claims(&self) -> &SharedClaims {
        &self.shared_claims
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchSubscriptionsChangedResponseAuth {
    #[serde(flatten)]
    pub shared_claims: SharedClaims,
    /// ksu - key server for identity key verification
    pub ksu: String,
    /// did:pkh
    pub sub: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionRequestAuth {
    #[serde(flatten)]
    pub shared_claims: SharedClaims,
    /// ksu - key server for identity key verification
    pub ksu: String,
    /// did:pkh
    pub sub: String,
    /// did:web of app domain
    pub app: String,
    /// space-delimited scope of notification types authorized by the user
    pub scp: String,
}

impl GetSharedClaims for SubscriptionRequestAuth {
    fn get_shared_claims(&self) -> &SharedClaims {
        &self.shared_claims
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionResponseAuth {
    #[serde(flatten)]
    pub shared_claims: SharedClaims,
    // FIXME change back to did:pkh
    /// publicKey
    pub sub: String,
    /// did:web of app domain
    pub app: String,
}

impl GetSharedClaims for SubscriptionResponseAuth {
    fn get_shared_claims(&self) -> &SharedClaims {
        &self.shared_claims
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionUpdateRequestAuth {
    #[serde(flatten)]
    pub shared_claims: SharedClaims,
    /// ksu - key server for identity key verification
    pub ksu: String,
    /// did:pkh
    pub sub: String,
    /// did:web of app domain
    pub app: String,
    /// space-delimited scope of notification types authorized by the user
    pub scp: String,
}

impl GetSharedClaims for SubscriptionUpdateRequestAuth {
    fn get_shared_claims(&self) -> &SharedClaims {
        &self.shared_claims
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionUpdateResponseAuth {
    #[serde(flatten)]
    pub shared_claims: SharedClaims,
    /// did:pkh
    pub sub: String,
    /// did:web of app domain
    pub app: String,
}

impl GetSharedClaims for SubscriptionUpdateResponseAuth {
    fn get_shared_claims(&self) -> &SharedClaims {
        &self.shared_claims
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionDeleteRequestAuth {
    #[serde(flatten)]
    pub shared_claims: SharedClaims,
    /// ksu - key server for identity key verification
    pub ksu: String,
    /// did:pkh
    pub sub: String,
    /// did:web of app domain
    pub app: String,
}

impl GetSharedClaims for SubscriptionDeleteRequestAuth {
    fn get_shared_claims(&self) -> &SharedClaims {
        &self.shared_claims
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionDeleteResponseAuth {
    #[serde(flatten)]
    pub shared_claims: SharedClaims,
    /// did:pkh
    pub sub: String,
    /// did:web of app domain
    pub app: String,
}

impl GetSharedClaims for SubscriptionDeleteResponseAuth {
    fn get_shared_claims(&self) -> &SharedClaims {
        &self.shared_claims
    }
}

// Workaround https://github.com/rust-lang/rust-clippy/issues/11613
#[allow(clippy::needless_return_with_question_mark)]
pub fn from_jwt<T: DeserializeOwned + GetSharedClaims>(jwt: &str) -> Result<T> {
    let mut parts = jwt.splitn(3, '.');
    let (Some(header), Some(claims)) = (parts.next(), parts.next()) else {
        return Err(AuthError::Format)?;
    };

    let header = base64::engine::general_purpose::STANDARD_NO_PAD.decode(header)?;
    let header = serde_json::from_slice::<JwtHeader>(&header)?;

    if header.alg != JWT_HEADER_ALG {
        Err(AuthError::Algorithm)?;
    }

    let claims = base64::engine::general_purpose::STANDARD_NO_PAD.decode(claims)?;
    let claims = serde_json::from_slice::<T>(&claims)?;

    if claims.get_shared_claims().exp < Utc::now().timestamp().unsigned_abs() {
        Err(AuthError::JwtExpired)?;
    }

    if claims.get_shared_claims().iat > Utc::now().timestamp_millis().unsigned_abs() {
        Err(AuthError::JwtNotYetValid)?;
    }

    let mut parts = jwt.rsplitn(2, '.');

    let (Some(signature), Some(message)) = (parts.next(), parts.next()) else {
        return Err(AuthError::Format)?;
    };

    // TODO if this is not a did:key no error happens until sig_result is checked
    // below

    let did_key = claims
        .get_shared_claims()
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
    )
    .map_err(AuthError::SignatureError)?;

    if sig_result {
        Ok(claims)
    } else {
        Err(AuthError::InvalidSignature)?
    }
}

pub fn sign_jwt<T: Serialize>(
    message: T,
    private_key: &ed25519_dalek::SigningKey,
) -> Result<String> {
    let header = {
        let data = JwtHeader {
            typ: JWT_HEADER_TYP,
            alg: JWT_HEADER_ALG,
        };
        let serialized = serde_json::to_string(&data)?;
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serialized)
    };

    let message = serde_json::to_string(&message)?;
    let message = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(message);
    let message = format!("{header}.{message}");
    let signature = private_key.sign(message.as_bytes());
    let signature = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature.to_bytes());

    Ok(format!("{message}.{signature}"))
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

    #[error("Signature error {0}")]
    SignatureError(jsonwebtoken::errors::Error),

    #[error("Invalid signature")]
    InvalidSignature,

    #[error("Invalid algorithm")]
    Algorithm,

    #[error("Keyserver returned non-success status code. status:{status} response:{response:?}")]
    KeyserverUnsuccessfulResponse {
        status: StatusCode,
        response: Response,
    },

    #[error("Keyserver returned non-success response. status:{status} error:{error:?}")]
    KeyserverNotSuccess {
        status: String,
        error: Option<Value>,
    },

    #[error("Keyserver returned successful response, but without a value")]
    KeyserverResponseMissingValue,

    #[error("JWT iss not did:key")]
    JwtIssNotDidKey,

    #[error("CACAO verification failed: {0}")]
    CacaoValidation(CacaoError),

    #[error("CACAO account doesn't match")]
    CacaoAccountMismatch,

    #[error("CACAO doesn't contain matching iss: {0}")]
    CacaoMissingIdentityKey(CacaoError),

    #[error("CACAO iss is not a did:pkh")]
    CacaoIssNotDidPkh,

    #[error("CACAO has wrong iss")]
    CacaoWrongIdentityKey,

    #[error("CACAO expired")]
    CacaoExpired,

    #[error("CACAO not yet valid")]
    CacaoNotYetValid,

    #[error("CACAO missing statement")]
    CacaoStatementMissing,

    #[error("CACAO invalid statement")]
    CacaoStatementInvalid,

    #[error("JWT expired")]
    JwtExpired,

    #[error("JWT not yet valid")]
    JwtNotYetValid,

    #[error("Invalid act")]
    InvalidAct,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Authorization {
    pub account: String,
    pub app: AuthorizedApp,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum AuthorizedApp {
    Limited(String),
    Unlimited,
}

pub async fn verify_identity(iss: &str, ksu: &str, sub: &str) -> Result<Authorization> {
    let mut url = Url::parse(ksu)?.join("/identity")?;
    let pubkey = iss
        .strip_prefix("did:key:")
        .ok_or(AuthError::JwtIssNotDidKey)?;
    url.set_query(Some(&format!("publicKey={pubkey}")));

    let response = reqwest::get(url).await?;
    if !response.status().is_success() {
        return Err(AuthError::KeyserverUnsuccessfulResponse {
            status: response.status(),
            response,
        })
        .map_err(Into::into);
    }

    let keyserver_response = response.json::<KeyServerResponse>().await?;

    if keyserver_response.status != "SUCCESS" {
        Err(AuthError::KeyserverNotSuccess {
            status: keyserver_response.status,
            error: keyserver_response.error,
        })?;
    }

    let Some(cacao) = keyserver_response.value else {
        // Keys server should never do this since it already returned SUCCESS above
        return Err(AuthError::KeyserverResponseMissingValue)?;
    };
    let cacao = cacao.cacao;

    let always_true = cacao.verify().map_err(AuthError::CacaoValidation)?;
    assert!(always_true);

    // TODO verify `cacao.p.aud`. Blocked by at least https://github.com/WalletConnect/walletconnect-utils/issues/128

    let cacao_identity_key = cacao
        .p
        .identity_key()
        .map_err(AuthError::CacaoMissingIdentityKey)?;
    if cacao_identity_key != pubkey {
        Err(AuthError::CacaoWrongIdentityKey)?;
    }

    let app = {
        let statement = cacao.p.statement.ok_or(AuthError::CacaoStatementMissing)?;
        if statement.contains("DAPP") || statement == STATEMENT_THIS_DOMAIN {
            AuthorizedApp::Limited(cacao.p.domain)
        } else if statement.contains("WALLET")
            || statement == STATEMENT
            || statement == STATEMENT_ALL_DOMAINS
        {
            AuthorizedApp::Unlimited
        } else {
            return Err(AuthError::CacaoStatementInvalid)?;
        }
    };

    if cacao.p.iss != sub {
        Err(AuthError::CacaoAccountMismatch)?;
    }

    let account = cacao
        .p
        .iss
        .strip_prefix("did:pkh:")
        .ok_or(AuthError::CacaoIssNotDidPkh)?
        .to_owned();

    if let Some(nbf) = cacao.p.nbf {
        let nbf = DateTime::parse_from_rfc3339(&nbf)?;

        if Utc::now().timestamp() <= nbf.timestamp() {
            Err(AuthError::CacaoNotYetValid)?;
        }
    }

    if let Some(exp) = cacao.p.exp {
        let exp = DateTime::parse_from_rfc3339(&exp)?;

        if exp.timestamp() <= Utc::now().timestamp() {
            Err(AuthError::CacaoExpired)?;
        }
    }

    Ok(Authorization { account, app })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KeyServerResponse {
    status: String,
    error: Option<Value>,
    value: Option<CacaoValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacaoValue {
    cacao: relay_rpc::auth::cacao::Cacao,
}
