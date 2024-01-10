use {
    crate::{
        error::{Error, Result},
        metrics::Metrics,
        model::{
            helpers::{GetNotificationsParams, GetNotificationsResult},
            types::AccountId,
        },
        registry::storage::{redis::Redis, KeyValueStorage},
    },
    base64::Engine,
    chrono::{DateTime, Duration as CDuration, Utc},
    core::fmt,
    ed25519_dalek::{Signer, SigningKey},
    hyper::StatusCode,
    relay_rpc::{
        auth::{
            cacao::{Cacao, CacaoError},
            did::{
                combine_did_data, extract_did_data, DidError, DID_DELIMITER, DID_METHOD_KEY,
                DID_PREFIX,
            },
        },
        domain::{ClientIdDecodingError, DecodedClientId},
        jwt::{JwtHeader, JWT_HEADER_ALG, JWT_HEADER_TYP},
    },
    reqwest::Response,
    serde::{de::DeserializeOwned, Deserialize, Serialize},
    serde_json::Value,
    std::{
        collections::HashSet,
        fmt::{Display, Formatter},
        sync::Arc,
        time::{Duration, Instant},
    },
    tracing::{debug, info, warn},
    url::Url,
    uuid::Uuid,
    validator::Validate,
    x25519_dalek::{PublicKey, StaticSecret},
};

pub const STATEMENT: &str = "I further authorize this app to send and receive messages on my behalf using my WalletConnect identity. Read more at https://walletconnect.com/identity";
pub const STATEMENT_ALL_DOMAINS_IDENTITY: &str = "I further authorize this app to send and receive messages on my behalf for ALL domains using my WalletConnect identity. Read more at https://walletconnect.com/identity";
pub const STATEMENT_THIS_DOMAIN_IDENTITY: &str = "I further authorize this app to send and receive messages on my behalf for THIS domain using my WalletConnect identity. Read more at https://walletconnect.com/identity";
pub const STATEMENT_ALL_DOMAINS_OLD: &str = "I further authorize this app to view and manage my notifications for ALL apps. Read more at https://walletconnect.com/notifications";
pub const STATEMENT_ALL_DOMAINS: &str = "I further authorize this app to view and manage my notifications for ALL apps. Read more at https://walletconnect.com/notifications-all-apps";
pub const STATEMENT_THIS_DOMAIN: &str = "I further authorize this app to send me notifications. Read more at https://walletconnect.com/notifications";

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
    /// major version of the API level being used as a string
    #[serde(default = "default_mjv")]
    pub mjv: String,
}

fn default_mjv() -> String {
    "0".to_owned()
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
    pub app: Option<DidWeb>,
}

impl GetSharedClaims for WatchSubscriptionsRequestAuth {
    fn get_shared_claims(&self) -> &SharedClaims {
        &self.shared_claims
    }
}

// TODO hard limit of max 1k subscriptions per user
// https://walletconnect.slack.com/archives/C044SKFKELR/p1699970394483349?thread_ts=1699969913.582709&cid=C044SKFKELR
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
    /// App domain that the subscription refers to
    pub app_domain: String,
    /// Authentication key used for authenticating topic JWTs and setting JWT aud field
    pub app_authentication_key: String,
    /// Symetric key used for notify topic. sha256 to get notify topic to manage
    /// the subscription and call wc_notifySubscriptionUpdate and
    /// wc_notifySubscriptionDelete
    pub sym_key: String,
    /// CAIP-10 account
    pub account: AccountId, // TODO do we need to return this?
    /// Array of notification types enabled for this subscription
    pub scope: HashSet<Uuid>, // TODO 15 hard limit
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

// Note: MessageAuth is different since it doesn't have `aud`
// pub struct MessageAuth {

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageResponseAuth {
    #[serde(flatten)]
    pub shared_claims: SharedClaims,
    /// ksu - key server for identity key verification
    pub ksu: String,
    /// did:pkh
    pub sub: String,
    /// did:web of app domain
    pub app: DidWeb,
}

impl GetSharedClaims for MessageResponseAuth {
    fn get_shared_claims(&self) -> &SharedClaims {
        &self.shared_claims
    }
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
    pub app: DidWeb,
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
    pub app: DidWeb,
    /// array of Notify Server Subscriptions
    pub sbs: Vec<NotifyServerSubscription>,
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
    pub app: DidWeb,
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
    pub app: DidWeb,
    /// array of Notify Server Subscriptions
    pub sbs: Vec<NotifyServerSubscription>,
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
    pub app: DidWeb,
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
    pub app: DidWeb,
    /// array of Notify Server Subscriptions
    pub sbs: Vec<NotifyServerSubscription>,
}

impl GetSharedClaims for SubscriptionDeleteResponseAuth {
    fn get_shared_claims(&self) -> &SharedClaims {
        &self.shared_claims
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct SubscriptionGetNotificationsRequestAuth {
    #[serde(flatten)]
    pub shared_claims: SharedClaims,
    /// ksu - key server for identity key verification
    pub ksu: String,
    /// did:pkh
    pub sub: String,
    /// did:web of app domain
    pub app: DidWeb,
    #[serde(flatten)]
    #[validate]
    pub params: GetNotificationsParams,
}

impl SubscriptionGetNotificationsRequestAuth {
    pub fn validate(&self) -> Result<()> {
        Validate::validate(&self).map_err(|error| Error::BadRequest(error.to_string()))
    }
}

impl GetSharedClaims for SubscriptionGetNotificationsRequestAuth {
    fn get_shared_claims(&self) -> &SharedClaims {
        &self.shared_claims
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionGetNotificationsResponseAuth {
    #[serde(flatten)]
    pub shared_claims: SharedClaims,
    /// did:pkh
    pub sub: String,
    /// did:web of app domain
    pub app: DidWeb,
    #[serde(flatten)]
    pub result: GetNotificationsResult,
}

impl GetSharedClaims for SubscriptionGetNotificationsResponseAuth {
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

    info!("iss: {}", claims.get_shared_claims().iss);

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

    #[error("JWT iss not did:key: {0}")]
    JwtIssNotDidKey(ClientIdDecodingError),

    #[error("CACAO verification failed: {0}")]
    CacaoValidation(CacaoError),

    #[error("CACAO account doesn't match")]
    CacaoAccountMismatch,

    #[error("CACAO doesn't contain matching iss: {0}")]
    CacaoMissingIdentityKey(CacaoError),

    #[error("CACAO iss is not a did:pkh: {0}")]
    CacaoIssNotDidPkh(DidError),

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
    pub account: AccountId,
    pub app: AuthorizedApp,
    pub domain: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub enum AuthorizedApp {
    Limited(String),
    Unlimited,
}

pub const KEYS_SERVER_STATUS_SUCCESS: &str = "SUCCESS";

async fn keys_server_request(url: Url) -> Result<Cacao> {
    info!("Timing: Requesting to keys server");
    let response = reqwest::get(url).await?;
    info!("Timing: Keys server response");

    if !response.status().is_success() {
        return Err(AuthError::KeyserverUnsuccessfulResponse {
            status: response.status(),
            response,
        }
        .into());
    }

    let keyserver_response = response.json::<KeyServerResponse>().await?;

    if keyserver_response.status != KEYS_SERVER_STATUS_SUCCESS {
        Err(AuthError::KeyserverNotSuccess {
            status: keyserver_response.status,
            error: keyserver_response.error,
        })?;
    }

    let Some(cacao) = keyserver_response.value else {
        // Keys server should never do this since it already returned SUCCESS above
        return Err(AuthError::KeyserverResponseMissingValue)?;
    };

    Ok(cacao.cacao)
}

async fn keys_server_request_cached(
    url: Url,
    redis: Option<&Arc<Redis>>,
) -> Result<(Cacao, KeysServerResponseSource)> {
    let cache_key = format!("keys-server-{}", url);

    if let Some(redis) = redis {
        let value = redis.get(&cache_key).await?;
        if let Some(cacao) = value {
            return Ok((cacao, KeysServerResponseSource::Cache));
        }
    }

    let cacao = keys_server_request(url).await?;

    if let Some(redis) = redis {
        let cacao = cacao.clone();
        let redis = redis.clone();
        let cache_ttl = chrono::Duration::weeks(1)
            .to_std()
            .expect("Static value should convert without error");
        // Do not block on cache write.
        tokio::spawn(async move {
            match redis.set(&cache_key, &cacao, Some(cache_ttl)).await {
                Ok(()) => debug!("Setting cache success"),
                Err(e) => {
                    warn!("failed to cache Keys Server response (cache_key:{cache_key}): {e:?}");
                }
            }
        });
    }

    Ok((cacao, KeysServerResponseSource::Server))
}

#[derive(Serialize, Clone)]
pub enum KeysServerResponseSource {
    Cache,
    Server,
}

impl KeysServerResponseSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Cache => "cache",
            Self::Server => "server",
        }
    }
}

pub const KEYS_SERVER_IDENTITY_ENDPOINT: &str = "/identity";
pub const KEYS_SERVER_IDENTITY_ENDPOINT_PUBLIC_KEY_QUERY: &str = "publicKey";

pub async fn verify_identity(
    iss_client_id: &DecodedClientId,
    ksu: &str,
    sub: &str,
    redis: Option<&Arc<Redis>>,
    metrics: Option<&Metrics>,
) -> Result<Authorization> {
    let mut url = Url::parse(ksu)?.join(KEYS_SERVER_IDENTITY_ENDPOINT)?;
    let pubkey = iss_client_id.to_string();
    url.query_pairs_mut()
        .append_pair(KEYS_SERVER_IDENTITY_ENDPOINT_PUBLIC_KEY_QUERY, &pubkey);

    let start = Instant::now();
    let (cacao, source) = keys_server_request_cached(url, redis).await?;
    if let Some(metrics) = metrics {
        metrics.keys_server_request(start, &source);
    }

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
        info!("CACAO statement: {statement}");
        parse_cacao_statement(&statement, &cacao.p.domain)?
    };

    if cacao.p.iss != sub {
        Err(AuthError::CacaoAccountMismatch)?;
    }

    let account = AccountId::from_did_pkh(&cacao.p.iss).map_err(AuthError::CacaoIssNotDidPkh)?;

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

    Ok(Authorization {
        account,
        app,
        domain: cacao.p.domain,
    })
}

fn parse_cacao_statement(statement: &str, domain: &str) -> Result<AuthorizedApp> {
    if statement.contains("DAPP")
        || statement == STATEMENT_THIS_DOMAIN_IDENTITY
        || statement == STATEMENT_THIS_DOMAIN
    {
        Ok(AuthorizedApp::Limited(domain.to_owned()))
    } else if statement.contains("WALLET")
        || statement == STATEMENT
        || statement == STATEMENT_ALL_DOMAINS_IDENTITY
        || statement == STATEMENT_ALL_DOMAINS_OLD
        || statement == STATEMENT_ALL_DOMAINS
    {
        Ok(AuthorizedApp::Unlimited)
    } else {
        return Err(AuthError::CacaoStatementInvalid)?;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyServerResponse {
    pub status: String,
    pub error: Option<Value>,
    pub value: Option<CacaoValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacaoValue {
    pub cacao: relay_rpc::auth::cacao::Cacao,
}

pub fn encode_authentication_private_key(authentication_key: &SigningKey) -> String {
    hex::encode(authentication_key.to_bytes())
}

pub fn encode_authentication_public_key(authentication_key: &SigningKey) -> String {
    hex::encode(authentication_key.verifying_key())
}

pub fn encode_subscribe_private_key(subscribe_key: &StaticSecret) -> String {
    hex::encode(subscribe_key)
}

pub fn encode_subscribe_public_key(subscribe_key: &StaticSecret) -> String {
    hex::encode(PublicKey::from(subscribe_key))
}

const DID_METHOD_WEB: &str = "web";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DidWeb {
    domain: Arc<str>,
}

impl DidWeb {
    pub fn from(did_web: &str) -> std::result::Result<Self, DidError> {
        let domain = extract_did_data(did_web, DID_METHOD_WEB)?;
        Ok(Self::from_domain(domain.to_owned()))
    }

    pub fn from_domain(domain: String) -> Self {
        Self::from_domain_arc(domain.into())
    }

    pub fn from_domain_arc(domain: Arc<str>) -> Self {
        // TODO domain validation?
        Self { domain }
    }

    pub fn domain(&self) -> &str {
        &self.domain
    }

    pub fn domain_arc(&self) -> Arc<str> {
        self.domain.clone()
    }

    pub fn into_domain(self) -> Arc<str> {
        self.domain
    }
}

impl Display for DidWeb {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", combine_did_data(DID_METHOD_WEB, &self.domain))
    }
}

impl Serialize for DidWeb {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'a> Deserialize<'a> for DidWeb {
    fn deserialize<D: serde::Deserializer<'a>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let did_web = String::deserialize(deserializer)?;
        Self::from(&did_web).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn notify_all_domains() {
        assert_eq!(
            parse_cacao_statement(STATEMENT_ALL_DOMAINS, "app.example.com").unwrap(),
            AuthorizedApp::Unlimited
        );
    }

    #[test]
    fn notify_all_domains_old() {
        assert_eq!(
            parse_cacao_statement(STATEMENT_ALL_DOMAINS_OLD, "app.example.com").unwrap(),
            AuthorizedApp::Unlimited
        );
    }

    #[test]
    fn notify_this_domain() {
        assert_eq!(
            parse_cacao_statement(STATEMENT_THIS_DOMAIN, "app.example.com").unwrap(),
            AuthorizedApp::Limited("app.example.com".to_owned())
        );
    }

    #[test]
    fn notify_all_domains_identity() {
        assert_eq!(
            parse_cacao_statement(STATEMENT_ALL_DOMAINS_IDENTITY, "app.example.com").unwrap(),
            AuthorizedApp::Unlimited
        );
    }

    #[test]
    fn notify_this_domain_identity() {
        assert_eq!(
            parse_cacao_statement(STATEMENT_THIS_DOMAIN_IDENTITY, "app.example.com").unwrap(),
            AuthorizedApp::Limited("app.example.com".to_owned())
        );
    }

    #[test]
    fn old_siwe_compatible() {
        assert_eq!(
            parse_cacao_statement(STATEMENT, "app.example.com").unwrap(),
            AuthorizedApp::Unlimited
        );
    }

    #[test]
    fn old_old_siwe_compatible() {
        assert_eq!(
            parse_cacao_statement(
                "I further authorize this DAPP to send and receive messages on my behalf for \
    this domain using my WalletConnect identity.",
                "app.example.com"
            )
            .unwrap(),
            AuthorizedApp::Limited("app.example.com".to_owned())
        );
    }
}
