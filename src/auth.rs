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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedClaims {
    /// iat - timestamp when jwt was issued
    pub iat: u64,
    /// exp - timestamp when jwt must expire
    pub exp: u64,
    /// iss - did:key of client identity key or dapp or Notify Server
    /// authentication key
    pub iss: String,
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
    /// description of action intent. Must be equal to
    /// "notify_watch_subscriptions"
    pub act: String,
    /// did:key of Notify Server authentication key
    pub aud: String,
    /// did:pkh of blockchain account that this request is associated with
    pub sub: String,
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
    /// description of action intent. Must be equal to
    /// "notify_watch_subscriptions_response"
    pub act: String,
    /// did:key of client identity key
    pub aud: String,
    /// did:pkh of blockchain account that this request is associated with
    pub sub: String,
    /// array of Notify Server Subscriptions
    pub sbs: Vec<NotifyServerSubscription>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotifyServerSubscription {
    /// dApp url that the subscription refers to
    pub dapp_url: String,
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
    /// description of action intent. Must be equal to
    /// "notify_subscriptions_changed"
    pub act: String,
    /// did:key of client identity key
    pub aud: String,
    /// did:pkh of blockchain account that this request is associated with
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
    /// description of action intent. Must be equal to
    /// "notify_subscriptions_changed_response"
    pub act: String,
    /// did:pkh of blockchain account that this request is associated with
    pub sub: String,
    /// did:key of Notify Server authentication key
    pub aud: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionRequestAuth {
    #[serde(flatten)]
    pub shared_claims: SharedClaims,
    /// ksu - key server for identity key verification
    pub ksu: String,
    /// description of action intent. Must be equal to "notify_subscription"
    pub act: String,
    /// did:key of an identity key. Enables to resolve associated Dapp domain
    /// used.
    pub aud: String,
    /// blockchain account that this notify subscription is associated with
    /// (did:pkh)
    pub sub: String,
    /// scope of notification types authorized by the user
    pub scp: String,
    /// dapp's domain url
    pub app: String,
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
    /// description of action intent. Must be equal to
    /// "notify_subscription_response"
    pub act: String,
    /// did:key of an identity key. Allows for the resolution of the attached
    /// blockchain account.
    pub aud: String,
    /// did:key of the public key used for key agreement on the Notify topic
    pub sub: String,
    /// dapp's domain url
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
    /// description of action intent. Must be equal to "notify_update"
    pub act: String,
    /// did:key of an identity key. Enables to resolve associated Dapp domain
    /// used.
    pub aud: String,
    /// did:pkh blockchain account that this notify subscription is associated
    /// with
    pub sub: String,
    /// scope of notification types authorized by the user
    pub scp: String,
    /// dapp's domain url
    pub app: String,
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
    /// description of action intent. Must be equal to "notify_update_response"
    pub act: String,
    /// did:key of an identity key. Enables to resolve attached blockchain
    /// account.
    pub aud: String,
    /// hash of the new subscription payload
    pub sub: String,
    /// dapp's domain url
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
    /// description of action intent. Must be equal to "notify_delete"
    pub act: String,
    /// did:key of an identity key. Enables to resolve associated Dapp domain
    /// used.
    pub aud: String,
    /// reason for deleting the subscription
    pub sub: String,
    /// dapp's domain url
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
    /// description of action intent. Must be equal to "notify_delete_response"
    pub act: String,
    /// did:key of an identity key. Enables to resolve attached blockchain
    /// account.
    pub aud: String,
    /// hash of the existing subscription payload
    pub sub: String,
    /// dapp's domain url
    pub app: String,
}

impl GetSharedClaims for SubscriptionDeleteResponseAuth {
    fn get_shared_claims(&self) -> &SharedClaims {
        &self.shared_claims
    }
}

pub fn from_jwt<T: DeserializeOwned + GetSharedClaims>(jwt: &str) -> Result<T> {
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
    let claims = serde_json::from_slice::<T>(&claims)?;

    if claims.get_shared_claims().exp < Utc::now().timestamp().unsigned_abs() {
        return Err(AuthError::JwtExpired)?;
    }

    if claims.get_shared_claims().iat > Utc::now().timestamp_millis().unsigned_abs() {
        return Err(AuthError::JwtNotYetValid)?;
    }

    let mut parts = jwt.rsplitn(2, '.');

    let (Some(signature), Some(message)) = (parts.next(), parts.next()) else {
        return Err(AuthError::Format)?;
    };

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
    );

    match sig_result {
        Ok(true) => Ok(claims),
        Ok(false) | Err(_) => Err(AuthError::InvalidSignature)?,
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

    let app = if true {
        AuthorizedApp::Unlimited
    } else {
        let statement = cacao.p.statement.ok_or(AuthError::CacaoStatementMissing)?;
        if statement.contains("DAPP") {
            AuthorizedApp::Limited(cacao.p.domain)
        } else if statement.contains("WALLET") {
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
