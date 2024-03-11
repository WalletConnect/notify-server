use {
    crate::{
        error::NotifyServerError,
        metrics::Metrics,
        model::{
            helpers::{GetNotificationsParams, GetNotificationsResult},
            types::{AccountId, AccountIdParseError},
        },
        registry::storage::{error::StorageError, redis::Redis, KeyValueStorage},
        siwx::{
            erc5573::{build_statement, parse_recap, RecapParseError, RECAP_URI_PREFIX},
            notify_recap::{
                ABILITY_ABILITY_ALL_APPS_MAGIC, ABILITY_ABILITY_SUFFIX, ABILITY_NAMESPACE_MANAGE,
                NOTIFY_URI,
            },
        },
    },
    base64::{DecodeError, Engine},
    chrono::{DateTime, Duration as CDuration, Utc},
    core::fmt,
    hyper::StatusCode,
    once_cell::sync::Lazy,
    regex::Regex,
    relay_rpc::{
        auth::{
            cacao::{signature::eip1271::get_rpc_url::GetRpcUrl, Cacao, CacaoError},
            did::{combine_did_data, extract_did_data, DidError},
            ed25519_dalek::{Signer, SigningKey},
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
    thiserror::Error,
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
    pub fn validate(&self) -> Result<(), NotifyServerError> {
        Validate::validate(&self)
            .map_err(|error| NotifyServerError::UnprocessableEntity(error.to_string()))
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

#[derive(Debug, Error)]
pub enum JwtError {
    #[error("Missing message part")]
    MissingMessage,

    #[error("Missing header part")]
    MissingHeader,

    #[error("Missing payload part")]
    MissingPayload,

    #[error("Missing signature part")]
    MissingSignature,

    #[error("Too many parts")]
    TooManyParts,

    #[error("Header not base64: {0}")]
    HeaderNotBase64(DecodeError),

    #[error("Payload not base64: {0}")]
    PayloadNotBase64(DecodeError),

    #[error("Header deserialization failed: {0}")]
    HeaderSerde(serde_json::error::Error),

    #[error("Payload deserialization failed: {0}")]
    PayloadSerde(serde_json::error::Error),

    #[error("Signature deserialization failed: {0}")]
    SignatureSerde(serde_json::error::Error),

    #[error("Unsupported algorithm")]
    UnsupportedAlgorithm,

    #[error("Expired")]
    Expired,

    #[error("Not yet valid")]
    NotYetValid,

    #[error("Issuer not did:key: {0}")]
    IssNotDidKey(ClientIdDecodingError),

    #[error("Signature verification error: {0}")]
    SignatureVerificationError(jsonwebtoken::errors::Error),

    #[error("Invalid signature")]
    InvalidSignature,
}

// Workaround https://github.com/rust-lang/rust-clippy/issues/11613
#[allow(clippy::needless_return_with_question_mark)]
pub fn from_jwt<T: DeserializeOwned + GetSharedClaims>(jwt: &str) -> Result<T, JwtError> {
    let mut message_signature_parts = jwt.rsplitn(2, '.');
    let signature_raw = message_signature_parts
        .next()
        .ok_or(JwtError::MissingSignature)?;
    let message_raw = message_signature_parts
        .next()
        .ok_or(JwtError::MissingMessage)?;
    if message_signature_parts.next().is_some() {
        return Err(JwtError::TooManyParts);
    }

    let mut header_payload_parts = message_raw.splitn(2, '.');
    let header_raw = header_payload_parts.next().ok_or(JwtError::MissingHeader)?;
    let payload_raw = header_payload_parts
        .next()
        .ok_or(JwtError::MissingPayload)?;
    if header_payload_parts.next().is_some() {
        return Err(JwtError::TooManyParts);
    }

    let header = base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(header_raw)
        .map_err(JwtError::HeaderNotBase64)?;
    let payload = base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(payload_raw)
        .map_err(JwtError::PayloadNotBase64)?;

    let header = serde_json::from_slice::<JwtHeader>(&header).map_err(JwtError::HeaderSerde)?;
    let claims = serde_json::from_slice::<T>(&payload).map_err(JwtError::PayloadSerde)?;

    if header.alg != JWT_HEADER_ALG {
        Err(JwtError::UnsupportedAlgorithm)?;
    }

    info!("iss: {}", claims.get_shared_claims().iss);

    let now = Utc::now();
    if claims.get_shared_claims().exp + 300 <= now.timestamp().unsigned_abs() {
        Err(JwtError::Expired)?;
    }
    if now.timestamp_millis().unsigned_abs() < claims.get_shared_claims().iat - 300 {
        Err(JwtError::NotYetValid)?;
    }

    let pub_key = DecodedClientId::try_from_did_key(&claims.get_shared_claims().iss)
        .map_err(JwtError::IssNotDidKey)?;
    let key = jsonwebtoken::DecodingKey::from_ed_der(pub_key.as_ref());

    let sig_result = jsonwebtoken::crypto::verify(
        signature_raw,
        message_raw.as_bytes(),
        &key,
        jsonwebtoken::Algorithm::EdDSA,
    )
    .map_err(JwtError::SignatureVerificationError)?;

    if sig_result {
        Ok(claims)
    } else {
        Err(JwtError::InvalidSignature)?
    }
}

pub fn sign_jwt<T: Serialize>(
    message: T,
    private_key: &SigningKey,
) -> Result<String, NotifyServerError> {
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

    #[error("JWT iss not did:key: {0}")]
    JwtIssNotDidKey(ClientIdDecodingError),

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

#[derive(Debug, Error)]
pub enum IdentityVerificationError {
    #[error("Client: {0}")]
    Client(#[from] IdentityVerificationClientError),

    #[error("Internal: {0}")]
    Internal(#[from] IdentityVerificationInternalError),
}

#[derive(Debug, Error)]
pub enum IdentityVerificationClientError {
    #[error("CACAO not registered")]
    NotRegistered,

    #[error("ksu could not be parsed as URL: {0}")]
    KsuNotUrl(url::ParseError),

    #[error("iss could not be parsed as an account ID: {0}")]
    CacaoAccountId(AccountIdParseError),

    #[error("CACAO failed verification: {0}")]
    CacaoVerification(CacaoError),

    #[error("CACAO doesn't contain matching iss: {0}")]
    CacaoMissingIdentityKey(CacaoError),

    #[error("CACAO has wrong iss")]
    CacaoWrongIdentityKey,

    #[error("CACAO missing statement")]
    CacaoStatementMissing,

    #[error("CACAO invalid statement")]
    CacaoStatementInvalid,

    #[error("CACAO account doesn't match")]
    CacaoAccountMismatch,

    #[error("CACAO not yet valid")]
    CacaoNotYetValid,

    #[error("CACAO nbf parse error: {0}")]
    CacaoNbfParse(chrono::ParseError),

    #[error("CACAO expired")]
    CacaoExpired,

    #[error("CACAO exp parse error: {0}")]
    CacaoExpParse(chrono::ParseError),

    #[error("CACAO recap more than one urn:recap URI in resources")]
    CacaoMoreThanOneRecapUri,

    #[error("CACAO recap parse error: {0}")]
    CacaoRecapParse(RecapParseError),

    #[error("CACAO statement does not match recap")]
    CacaoStatementDoesNotMatchRecap,

    #[error("CACAO recap does not have Notify URI")]
    CacaoRecapNoNotifyUri,

    #[error("CACAO recap does not have any abilities")]
    CacaoRecapNoAbilities,

    #[error("CACAO recap should only have one ability")]
    CacaoRecapMoreThanOneAbility,

    #[error("CACAO recap ability namepsace is not `manage`")]
    CacaoRecapAbilityNamespaceNotManage,

    #[error("CACAO recap ability name does not end with `-notifications`")]
    CacaoRecapAbilityNameNotNotifications,

    #[error("CACAO recap ability has empty array with no objects which implies that there is no way to use this ability in a valid way")]
    CacaoRecapAbilityEmptyObjects,

    #[error("CACAO recap ability has more than one object which is not valid")]
    CacaoRecapAbilityMoreThanOneObject,

    #[error("CACAO recap ability has a non-empty object which is not valid")]
    CacaoRecapAbilityNonEmptyObject,

    #[error("CACAO recap ability name is not magic `all-apps` but is not a valid domain")]
    CacaoRecapAbilityNameNotValidDomain,
}

#[derive(Debug, Error)]
pub enum IdentityVerificationInternalError {
    #[error("HTTP: {0}")]
    Http(reqwest::Error),

    #[error("JSON: {0}")]
    Json(reqwest::Error),

    #[error("Keys Server returned non-success response. status:{status} error:{error:?}")]
    KeyServerNotSuccess {
        status: String,
        error: Option<Value>,
    },

    #[error("Keys Server returned successful response, but without a value")]
    KeyServerResponseMissingValue,

    #[error("Keys Server returned non-success status code. status:{status} response:{response:?}")]
    KeyServerUnsuccessfulResponse {
        status: StatusCode,
        response: Response,
    },

    #[error("Cache lookup: {0}")]
    CacheLookup(StorageError),

    #[error("Could not construct Keys Server request URL: {0}")]
    KeysServerRequestUrlConstructionError(url::ParseError),
}

pub const KEYS_SERVER_STATUS_SUCCESS: &str = "SUCCESS";

async fn keys_server_request(url: Url) -> Result<Cacao, IdentityVerificationError> {
    info!("Timing: Requesting to keys server");
    let response = reqwest::get(url)
        .await
        .map_err(IdentityVerificationInternalError::Http)?;
    info!("Timing: Keys server response");

    match response.status() {
        StatusCode::NOT_FOUND => Err(IdentityVerificationClientError::NotRegistered)?,
        status if status.is_success() => {
            let keyserver_response = response
                .json::<KeyServerResponse>()
                .await
                .map_err(IdentityVerificationInternalError::Json)?;

            if keyserver_response.status != KEYS_SERVER_STATUS_SUCCESS {
                Err(IdentityVerificationInternalError::KeyServerNotSuccess {
                    status: keyserver_response.status,
                    error: keyserver_response.error,
                })?;
            }

            let Some(cacao) = keyserver_response.value else {
                // Keys server should never do this since it already returned SUCCESS above
                return Err(IdentityVerificationInternalError::KeyServerResponseMissingValue)?;
            };

            Ok(cacao.cacao)
        }
        status => Err(
            IdentityVerificationInternalError::KeyServerUnsuccessfulResponse { status, response },
        )?,
    }
}

async fn keys_server_request_cached(
    url: Url,
    redis: Option<&Arc<Redis>>,
) -> Result<(Cacao, KeysServerResponseSource), IdentityVerificationError> {
    let cache_key = format!("keys-server-{}", url);

    if let Some(redis) = redis {
        let value = redis
            .get(&cache_key)
            .await
            .map_err(IdentityVerificationInternalError::CacheLookup)?;
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
                    // TODO metrics & alarm
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
    provider: Option<&impl GetRpcUrl>,
    metrics: Option<&Metrics>,
) -> Result<Authorization, IdentityVerificationError> {
    let mut url = Url::parse(ksu)
        .map_err(IdentityVerificationClientError::KsuNotUrl)?
        .join(KEYS_SERVER_IDENTITY_ENDPOINT)
        // This probably shouldn't error, but catching just in-case
        .map_err(IdentityVerificationInternalError::KeysServerRequestUrlConstructionError)?;
    let pubkey = iss_client_id.to_string();
    url.query_pairs_mut()
        .append_pair(KEYS_SERVER_IDENTITY_ENDPOINT_PUBLIC_KEY_QUERY, &pubkey);

    let start = Instant::now();
    let (cacao, source) = keys_server_request_cached(url, redis).await?;
    if let Some(metrics) = metrics {
        metrics.keys_server_request(start, &source);
    }

    let account = AccountId::from_did_pkh(&cacao.p.iss)
        .map_err(IdentityVerificationClientError::CacaoAccountId)?;

    let always_true = cacao
        .verify(provider)
        .await
        .map_err(IdentityVerificationClientError::CacaoVerification)?;
    assert!(always_true);

    // TODO verify `cacao.p.aud`. Blocked by at least https://github.com/WalletConnect/walletconnect-utils/issues/128

    let cacao_identity_key = cacao
        .p
        .identity_key()
        .map_err(IdentityVerificationClientError::CacaoMissingIdentityKey)?;
    if cacao_identity_key != pubkey {
        Err(IdentityVerificationClientError::CacaoWrongIdentityKey)?;
    }

    let app = {
        let statement = cacao
            .p
            .statement
            .ok_or(IdentityVerificationClientError::CacaoStatementMissing)?;
        info!("CACAO statement: {statement}");

        let resources = cacao.p.resources.unwrap_or(vec![]);

        // As per the spec, there may be at most one recap URI in the spec
        if resources
            .iter()
            .filter(|uri| uri.starts_with(RECAP_URI_PREFIX))
            .count()
            > 1
        {
            Err(IdentityVerificationClientError::CacaoMoreThanOneRecapUri)?;
        }

        // As per the spec, the last resource must be the recap, if recaps are in-use
        let recap_uri = resources.last().map(String::as_str);
        let recap =
            parse_recap(recap_uri).map_err(IdentityVerificationClientError::CacaoRecapParse)?;

        if let Some(recap) = recap {
            let expected_statement_suffix = build_statement(&recap);
            if !statement.ends_with(&expected_statement_suffix) {
                Err(IdentityVerificationClientError::CacaoStatementDoesNotMatchRecap)?;
            }

            // Expect one ability for our Notify URI
            let abilities = recap
                .att
                .get(NOTIFY_URI)
                .ok_or(IdentityVerificationClientError::CacaoRecapNoNotifyUri)?;
            if abilities.len() > 1 {
                Err(IdentityVerificationClientError::CacaoRecapMoreThanOneAbility)?;
            }
            let (ability, objects) = abilities
                .iter()
                .next()
                .ok_or(IdentityVerificationClientError::CacaoRecapNoAbilities)?;

            // Validate correct namespace
            if ability.namespace != ABILITY_NAMESPACE_MANAGE {
                Err(IdentityVerificationClientError::CacaoRecapAbilityNamespaceNotManage)?;
            }

            // Validate correct name
            let maybe_domain =
                if let Some(prefix) = ability.name.strip_suffix(ABILITY_ABILITY_SUFFIX) {
                    prefix
                } else {
                    return Err(
                        IdentityVerificationClientError::CacaoRecapAbilityNameNotNotifications,
                    )?;
                };

            // Validate objects
            if objects.len() > 1 {
                Err(IdentityVerificationClientError::CacaoRecapAbilityMoreThanOneObject)?;
            }
            let object = objects
                .iter()
                .next()
                .ok_or(IdentityVerificationClientError::CacaoRecapAbilityEmptyObjects)?;
            if object != &Value::Object(serde_json::Map::new()) {
                Err(IdentityVerificationClientError::CacaoRecapAbilityNonEmptyObject)?;
            }

            // Intrepret ability
            if maybe_domain == ABILITY_ABILITY_ALL_APPS_MAGIC {
                AuthorizedApp::Unlimited
            } else {
                static DOMAIN_REGEX: Lazy<Regex> = Lazy::new(|| {
                    Regex::new(r"^[a-zA-Z0-9.-]+$").expect("Error should be caught in test cases")
                });
                if DOMAIN_REGEX.captures(maybe_domain).is_some() {
                    AuthorizedApp::Limited(maybe_domain.to_owned())
                } else {
                    return Err(
                        IdentityVerificationClientError::CacaoRecapAbilityNameNotValidDomain,
                    )?;
                }
            }
        } else {
            parse_cacao_statement(&statement, &cacao.p.domain)
                .map_err(|_| IdentityVerificationClientError::CacaoStatementInvalid)?
        }
    };

    if cacao.p.iss != sub {
        Err(IdentityVerificationClientError::CacaoAccountMismatch)?;
    }

    if let Some(nbf) = cacao.p.nbf {
        let nbf = DateTime::parse_from_rfc3339(&nbf)
            .map_err(IdentityVerificationClientError::CacaoNbfParse)?;
        if Utc::now().timestamp() <= nbf.timestamp() {
            return Err(IdentityVerificationClientError::CacaoNotYetValid)?;
        }
    }

    if let Some(exp) = cacao.p.exp {
        let exp = DateTime::parse_from_rfc3339(&exp)
            .map_err(IdentityVerificationClientError::CacaoExpParse)?;
        if exp.timestamp() <= Utc::now().timestamp() {
            return Err(IdentityVerificationClientError::CacaoExpired)?;
        }
    }

    Ok(Authorization {
        account,
        app,
        domain: cacao.p.domain,
    })
}

fn parse_cacao_statement(statement: &str, domain: &str) -> Result<AuthorizedApp, ()> {
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
        Err(())
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
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'a> Deserialize<'a> for DidWeb {
    fn deserialize<D: serde::Deserializer<'a>>(deserializer: D) -> Result<Self, D::Error> {
        let did_web = String::deserialize(deserializer)?;
        Self::from(&did_web).map_err(serde::de::Error::custom)
    }
}

pub mod test_utils {
    use {
        super::{
            AccountId, CacaoValue, DidWeb, KeyServerResponse, KEYS_SERVER_IDENTITY_ENDPOINT,
            KEYS_SERVER_IDENTITY_ENDPOINT_PUBLIC_KEY_QUERY, KEYS_SERVER_STATUS_SUCCESS,
        },
        crate::siwx::{
            erc5573::{build_statement, test_utils::encode_recaip_uri},
            notify_recap::test_utils::build_recap_details_object,
        },
        chrono::Utc,
        hyper::StatusCode,
        k256::ecdsa::SigningKey as EcdsaSigningKey,
        relay_rpc::{
            auth::{
                cacao::{
                    self,
                    header::EIP4361,
                    signature::{
                        eip1271::get_rpc_url::GetRpcUrl,
                        eip191::{eip191_bytes, EIP191},
                    },
                    Cacao,
                },
                ed25519_dalek::SigningKey as Ed25519SigningKey,
            },
            domain::DecodedClientId,
        },
        sha2::Digest,
        sha3::Keccak256,
        url::Url,
        wiremock::{
            http::Method,
            matchers::{method, path, query_param},
            Mock, MockServer, ResponseTemplate,
        },
    };

    #[derive(Clone)]
    pub struct IdentityKeyDetails {
        pub keys_server_url: Url,
        pub signing_key: Ed25519SigningKey,
        pub client_id: DecodedClientId,
    }

    pub fn generate_identity_key() -> (Ed25519SigningKey, DecodedClientId) {
        let signing_key = Ed25519SigningKey::generate(&mut rand::thread_rng());
        let client_id = DecodedClientId::from_key(&signing_key.verifying_key());
        (signing_key, client_id)
    }

    #[derive(Debug, PartialEq, Eq)]
    pub enum CacaoAuth {
        Statement(String),
        AllApps,
        ThisApp,
    }

    pub async fn sign_cacao(
        app_domain: &DidWeb,
        account: &AccountId,
        auth: CacaoAuth,
        identity_public_key: DecodedClientId,
        keys_server_url: String,
        account_signing_key: &EcdsaSigningKey,
    ) -> cacao::Cacao {
        let did_key = identity_public_key.to_did_key();
        let mut resources = vec![keys_server_url];
        let (statement, aud) = match auth {
            CacaoAuth::Statement(statement) => (statement, did_key),
            CacaoAuth::AllApps | CacaoAuth::ThisApp => {
                let domain = app_domain.domain();
                let recap = build_recap_details_object(if auth == CacaoAuth::ThisApp {
                    Some(domain)
                } else {
                    None
                });
                resources.push(encode_recaip_uri(&recap));
                let statement = build_statement(&recap);
                let aud = {
                    let mut url = format!("https://{domain}").parse::<Url>().unwrap();
                    url.query_pairs_mut().append_pair(
                        cacao::payload::Payload::WALLETCONNECT_IDENTITY_KEY,
                        &did_key,
                    );
                    url.to_string()
                };
                (statement, aud)
            }
        };
        let mut cacao = cacao::Cacao {
            h: cacao::header::Header {
                t: EIP4361.to_owned().into(),
            },
            p: cacao::payload::Payload {
                domain: app_domain.domain().to_owned(),
                iss: account.to_did_pkh(),
                statement: Some(statement),
                aud,
                version: cacao::Version::V1,
                nonce: hex::encode(rand::Rng::gen::<[u8; 10]>(&mut rand::thread_rng())),
                iat: Utc::now().to_rfc3339(),
                exp: None,
                nbf: None,
                request_id: None,
                resources: Some(resources),
            },
            s: cacao::signature::Signature {
                t: "".to_owned(),
                s: "".to_owned(),
            },
        };
        let (signature, recovery): (k256::ecdsa::Signature, _) = account_signing_key
            .sign_digest_recoverable(Keccak256::new_with_prefix(eip191_bytes(
                &cacao.siwe_message().unwrap(),
            )))
            .unwrap();
        let cacao_signature = [&signature.to_bytes()[..], &[recovery.to_byte()]].concat();
        cacao.s.t = EIP191.to_owned();
        cacao.s.s = hex::encode(cacao_signature);
        cacao.verify(Some(&MockGetRpcUrl)).await.unwrap();
        cacao
    }

    pub struct MockGetRpcUrl;
    impl GetRpcUrl for MockGetRpcUrl {
        async fn get_rpc_url(&self, _: String) -> Option<Url> {
            None
        }
    }

    pub async fn register_mocked_identity_key(
        mock_keys_server: &MockServer,
        identity_public_key: DecodedClientId,
        cacao: Cacao,
    ) {
        Mock::given(method(Method::Get))
            .and(path(KEYS_SERVER_IDENTITY_ENDPOINT))
            .and(query_param(
                KEYS_SERVER_IDENTITY_ENDPOINT_PUBLIC_KEY_QUERY,
                identity_public_key.to_string(),
            ))
            .respond_with(
                ResponseTemplate::new(StatusCode::OK).set_body_json(KeyServerResponse {
                    status: KEYS_SERVER_STATUS_SUCCESS.to_owned(),
                    error: None,
                    value: Some(CacaoValue { cacao }),
                }),
            )
            .mount(mock_keys_server)
            .await;
    }
}

#[cfg(test)]
pub mod test {
    use {
        super::*,
        crate::{
            auth::test_utils::{
                generate_identity_key, register_mocked_identity_key, sign_cacao, IdentityKeyDetails,
            },
            model::types::eip155::test_utils::generate_account,
            siwx::{
                erc5573::{test_utils::encode_recaip_uri, Ability, ReCapDetailsObject},
                notify_recap::{
                    test_utils::build_recap_details_object, ABILITY_ABILITY_ALL_APPS_MAGIC,
                    ABILITY_ABILITY_SUFFIX, ABILITY_NAMESPACE_MANAGE, NOTIFY_URI,
                },
            },
        },
        relay_rpc::{
            auth::cacao::{
                self,
                header::EIP4361,
                signature::eip191::{eip191_bytes, EIP191},
            },
            domain::ProjectId,
        },
        sha2::Digest,
        sha3::Keccak256,
        std::collections::HashMap,
        test::test_utils::{CacaoAuth, MockGetRpcUrl},
        wiremock::{
            http::Method,
            matchers::{method, path, query_param},
            Mock, MockServer, ResponseTemplate,
        },
    };

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

    #[tokio::test]
    async fn test_keys_server_request() {
        let (account_signing_key, account) = generate_account();

        let keys_server = MockServer::start().await;
        let keys_server_url = keys_server.uri().parse::<Url>().unwrap();

        let (identity_signing_key, identity_public_key) = generate_identity_key();
        let identity_key_details = IdentityKeyDetails {
            keys_server_url: keys_server_url.clone(),
            signing_key: identity_signing_key,
            client_id: identity_public_key.clone(),
        };

        let project_id = ProjectId::generate();
        let app_domain = DidWeb::from_domain(format!("{project_id}.walletconnect.com"));

        let cacao = sign_cacao(
            &app_domain,
            &account,
            CacaoAuth::Statement(STATEMENT_THIS_DOMAIN.to_owned()),
            identity_public_key.clone(),
            identity_key_details.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await;

        register_mocked_identity_key(&keys_server, identity_public_key.clone(), cacao.clone())
            .await;

        let mut keys_server_request_url =
            keys_server_url.join(KEYS_SERVER_IDENTITY_ENDPOINT).unwrap();
        keys_server_request_url.query_pairs_mut().append_pair(
            KEYS_SERVER_IDENTITY_ENDPOINT_PUBLIC_KEY_QUERY,
            &identity_public_key.to_string(),
        );
        assert_eq!(
            cacao,
            keys_server_request(keys_server_request_url).await.unwrap()
        );
    }

    #[tokio::test]
    async fn test_keys_server_request_404() {
        let identity_public_key = Uuid::new_v4();

        let keys_server = MockServer::start().await;
        let keys_server_url = keys_server.uri().parse::<Url>().unwrap();

        Mock::given(method(Method::Get))
            .and(path(KEYS_SERVER_IDENTITY_ENDPOINT))
            .and(query_param(
                KEYS_SERVER_IDENTITY_ENDPOINT_PUBLIC_KEY_QUERY,
                identity_public_key.to_string(),
            ))
            .respond_with(ResponseTemplate::new(StatusCode::NOT_FOUND))
            .mount(&keys_server)
            .await;

        let mut keys_server_request_url =
            keys_server_url.join(KEYS_SERVER_IDENTITY_ENDPOINT).unwrap();
        keys_server_request_url.query_pairs_mut().append_pair(
            KEYS_SERVER_IDENTITY_ENDPOINT_PUBLIC_KEY_QUERY,
            &identity_public_key.to_string(),
        );
        assert!(matches!(
            keys_server_request(keys_server_request_url).await,
            Err(IdentityVerificationError::Client(
                IdentityVerificationClientError::NotRegistered
            ))
        ));
    }

    #[tokio::test]
    async fn test_keys_server_request_500() {
        let identity_public_key = Uuid::new_v4();

        let keys_server = MockServer::start().await;
        let keys_server_url = keys_server.uri().parse::<Url>().unwrap();

        Mock::given(method(Method::Get))
            .and(path(KEYS_SERVER_IDENTITY_ENDPOINT))
            .and(query_param(
                KEYS_SERVER_IDENTITY_ENDPOINT_PUBLIC_KEY_QUERY,
                identity_public_key.to_string(),
            ))
            .respond_with(ResponseTemplate::new(StatusCode::INTERNAL_SERVER_ERROR))
            .mount(&keys_server)
            .await;

        let mut keys_server_request_url =
            keys_server_url.join(KEYS_SERVER_IDENTITY_ENDPOINT).unwrap();
        keys_server_request_url.query_pairs_mut().append_pair(
            KEYS_SERVER_IDENTITY_ENDPOINT_PUBLIC_KEY_QUERY,
            &identity_public_key.to_string(),
        );
        assert!(matches!(
            keys_server_request(keys_server_request_url).await,
            Err(IdentityVerificationError::Internal(
                IdentityVerificationInternalError::KeyServerUnsuccessfulResponse {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    response: _
                }
            ))
        ));
    }

    #[tokio::test]
    async fn statement_this_app() {
        let (account_signing_key, account) = generate_account();

        let keys_server = MockServer::start().await;
        let keys_server_url = keys_server.uri().parse::<Url>().unwrap();

        let (identity_signing_key, identity_public_key) = generate_identity_key();
        let identity_key_details = IdentityKeyDetails {
            keys_server_url: keys_server_url.clone(),
            signing_key: identity_signing_key,
            client_id: identity_public_key.clone(),
        };

        let project_id = ProjectId::generate();
        let app_domain = DidWeb::from_domain(format!("{project_id}.walletconnect.com"));

        let cacao = sign_cacao(
            &app_domain,
            &account,
            CacaoAuth::Statement(STATEMENT_THIS_DOMAIN.to_owned()),
            identity_public_key.clone(),
            identity_key_details.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await;

        register_mocked_identity_key(&keys_server, identity_public_key.clone(), cacao.clone())
            .await;

        let Authorization {
            account,
            app,
            domain,
        } = verify_identity(
            &identity_public_key,
            keys_server_url.as_ref(),
            &account.to_did_pkh(),
            None,
            Some(&MockGetRpcUrl),
            None,
        )
        .await
        .unwrap();
        assert_eq!(account, account);
        assert_eq!(domain, app_domain.domain());
        assert_eq!(app, AuthorizedApp::Limited(app_domain.domain().to_owned()));
    }

    #[tokio::test]
    async fn statement_all_apps() {
        let (account_signing_key, account) = generate_account();

        let keys_server = MockServer::start().await;
        let keys_server_url = keys_server.uri().parse::<Url>().unwrap();

        let (identity_signing_key, identity_public_key) = generate_identity_key();
        let identity_key_details = IdentityKeyDetails {
            keys_server_url: keys_server_url.clone(),
            signing_key: identity_signing_key,
            client_id: identity_public_key.clone(),
        };

        let project_id = ProjectId::generate();
        let app_domain = DidWeb::from_domain(format!("{project_id}.walletconnect.com"));

        let cacao = sign_cacao(
            &app_domain,
            &account,
            CacaoAuth::Statement(STATEMENT_ALL_DOMAINS.to_owned()),
            identity_public_key.clone(),
            identity_key_details.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await;

        register_mocked_identity_key(&keys_server, identity_public_key.clone(), cacao.clone())
            .await;

        let Authorization {
            account,
            app,
            domain,
        } = verify_identity(
            &identity_public_key,
            keys_server_url.as_ref(),
            &account.to_did_pkh(),
            None,
            Some(&MockGetRpcUrl),
            None,
        )
        .await
        .unwrap();
        assert_eq!(account, account);
        assert_eq!(domain, app_domain.domain());
        assert_eq!(app, AuthorizedApp::Unlimited);
    }

    #[tokio::test]
    async fn recaps_this_app() {
        let (account_signing_key, account) = generate_account();

        let keys_server = MockServer::start().await;
        let keys_server_url = keys_server.uri().parse::<Url>().unwrap();

        let (identity_signing_key, identity_public_key) = generate_identity_key();
        let identity_key_details = IdentityKeyDetails {
            keys_server_url: keys_server_url.clone(),
            signing_key: identity_signing_key,
            client_id: identity_public_key.clone(),
        };

        let project_id = ProjectId::generate();
        let app_domain = DidWeb::from_domain(format!("{project_id}.walletconnect.com"));

        let cacao = sign_cacao(
            &app_domain,
            &account,
            CacaoAuth::ThisApp,
            identity_public_key.clone(),
            identity_key_details.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await;

        register_mocked_identity_key(&keys_server, identity_public_key.clone(), cacao.clone())
            .await;

        let Authorization {
            account,
            app,
            domain,
        } = verify_identity(
            &identity_public_key,
            keys_server_url.as_ref(),
            &account.to_did_pkh(),
            None,
            Some(&MockGetRpcUrl),
            None,
        )
        .await
        .unwrap();
        assert_eq!(account, account);
        assert_eq!(domain, app_domain.domain());
        assert_eq!(app, AuthorizedApp::Limited(app_domain.domain().to_owned()));
    }

    #[tokio::test]
    async fn recaps_all_apps() {
        let (account_signing_key, account) = generate_account();

        let keys_server = MockServer::start().await;
        let keys_server_url = keys_server.uri().parse::<Url>().unwrap();

        let (identity_signing_key, identity_public_key) = generate_identity_key();
        let identity_key_details = IdentityKeyDetails {
            keys_server_url: keys_server_url.clone(),
            signing_key: identity_signing_key,
            client_id: identity_public_key.clone(),
        };

        let project_id = ProjectId::generate();
        let app_domain = DidWeb::from_domain(format!("{project_id}.walletconnect.com"));

        let cacao = sign_cacao(
            &app_domain,
            &account,
            CacaoAuth::AllApps,
            identity_public_key.clone(),
            identity_key_details.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await;

        register_mocked_identity_key(&keys_server, identity_public_key.clone(), cacao.clone())
            .await;

        let Authorization {
            account,
            app,
            domain,
        } = verify_identity(
            &identity_public_key,
            keys_server_url.as_ref(),
            &account.to_did_pkh(),
            None,
            Some(&MockGetRpcUrl),
            None,
        )
        .await
        .unwrap();
        assert_eq!(account, account);
        assert_eq!(domain, app_domain.domain());
        assert_eq!(app, AuthorizedApp::Unlimited);
    }

    #[tokio::test]
    async fn recaps_fail_more_than_one_recap_uri() {
        let (account_signing_key, account) = generate_account();

        let keys_server = MockServer::start().await;
        let keys_server_url = keys_server.uri().parse::<Url>().unwrap();

        let (_identity_signing_key, identity_public_key) = generate_identity_key();

        let project_id = ProjectId::generate();
        let app_domain = DidWeb::from_domain(format!("{project_id}.walletconnect.com"));

        let cacao = {
            let did_key = identity_public_key.to_did_key();
            let mut resources = vec![keys_server_url.to_string()];
            let domain = app_domain.domain();
            let recap = build_recap_details_object(Some(app_domain.domain()));
            resources.push(encode_recaip_uri(&recap));
            resources.push(encode_recaip_uri(&recap));
            let statement = build_statement(&recap);
            let aud = {
                let mut url = format!("https://{domain}").parse::<Url>().unwrap();
                url.query_pairs_mut().append_pair(
                    cacao::payload::Payload::WALLETCONNECT_IDENTITY_KEY,
                    &did_key,
                );
                url.to_string()
            };
            let mut cacao = cacao::Cacao {
                h: cacao::header::Header {
                    t: EIP4361.to_owned().into(),
                },
                p: cacao::payload::Payload {
                    domain: app_domain.domain().to_owned(),
                    iss: account.to_did_pkh(),
                    statement: Some(statement),
                    aud,
                    version: cacao::Version::V1,
                    nonce: hex::encode(rand::Rng::gen::<[u8; 10]>(&mut rand::thread_rng())),
                    iat: Utc::now().to_rfc3339(),
                    exp: None,
                    nbf: None,
                    request_id: None,
                    resources: Some(resources),
                },
                s: cacao::signature::Signature {
                    t: "".to_owned(),
                    s: "".to_owned(),
                },
            };
            let (signature, recovery): (k256::ecdsa::Signature, _) = account_signing_key
                .sign_digest_recoverable(Keccak256::new_with_prefix(eip191_bytes(
                    &cacao.siwe_message().unwrap(),
                )))
                .unwrap();
            let cacao_signature = [&signature.to_bytes()[..], &[recovery.to_byte()]].concat();
            cacao.s.t = EIP191.to_owned();
            cacao.s.s = hex::encode(cacao_signature);
            cacao.verify(Some(&MockGetRpcUrl)).await.unwrap();
            cacao
        };

        register_mocked_identity_key(&keys_server, identity_public_key.clone(), cacao.clone())
            .await;

        let result = verify_identity(
            &identity_public_key,
            keys_server_url.as_ref(),
            &account.to_did_pkh(),
            None,
            Some(&MockGetRpcUrl),
            None,
        )
        .await;
        assert!(matches!(
            result,
            Err(IdentityVerificationError::Client(
                IdentityVerificationClientError::CacaoMoreThanOneRecapUri
            ))
        ));
    }

    #[tokio::test]
    async fn recaps_fail_empty_array() {
        let (account_signing_key, account) = generate_account();

        let keys_server = MockServer::start().await;
        let keys_server_url = keys_server.uri().parse::<Url>().unwrap();

        let (_identity_signing_key, identity_public_key) = generate_identity_key();

        let project_id = ProjectId::generate();
        let app_domain = DidWeb::from_domain(format!("{project_id}.walletconnect.com"));

        let cacao = {
            let did_key = identity_public_key.to_did_key();
            let mut resources = vec![keys_server_url.to_string()];
            let domain = app_domain.domain();
            let recap = ReCapDetailsObject {
                att: HashMap::from_iter([(
                    NOTIFY_URI.to_owned(),
                    HashMap::from_iter([(
                        Ability {
                            namespace: ABILITY_NAMESPACE_MANAGE.to_owned(),
                            name: format!(
                                "{ABILITY_ABILITY_ALL_APPS_MAGIC}{ABILITY_ABILITY_SUFFIX}",
                            ),
                        },
                        vec![],
                    )]),
                )]),
            };
            resources.push(encode_recaip_uri(&recap));
            let statement = build_statement(&recap);
            let aud = {
                let mut url = format!("https://{domain}").parse::<Url>().unwrap();
                url.query_pairs_mut().append_pair(
                    cacao::payload::Payload::WALLETCONNECT_IDENTITY_KEY,
                    &did_key,
                );
                url.to_string()
            };
            let mut cacao = cacao::Cacao {
                h: cacao::header::Header {
                    t: EIP4361.to_owned().into(),
                },
                p: cacao::payload::Payload {
                    domain: app_domain.domain().to_owned(),
                    iss: account.to_did_pkh(),
                    statement: Some(statement),
                    aud,
                    version: cacao::Version::V1,
                    nonce: hex::encode(rand::Rng::gen::<[u8; 10]>(&mut rand::thread_rng())),
                    iat: Utc::now().to_rfc3339(),
                    exp: None,
                    nbf: None,
                    request_id: None,
                    resources: Some(resources),
                },
                s: cacao::signature::Signature {
                    t: "".to_owned(),
                    s: "".to_owned(),
                },
            };
            let (signature, recovery): (k256::ecdsa::Signature, _) = account_signing_key
                .sign_digest_recoverable(Keccak256::new_with_prefix(eip191_bytes(
                    &cacao.siwe_message().unwrap(),
                )))
                .unwrap();
            let cacao_signature = [&signature.to_bytes()[..], &[recovery.to_byte()]].concat();
            cacao.s.t = EIP191.to_owned();
            cacao.s.s = hex::encode(cacao_signature);
            cacao.verify(Some(&MockGetRpcUrl)).await.unwrap();
            cacao
        };

        register_mocked_identity_key(&keys_server, identity_public_key.clone(), cacao.clone())
            .await;

        let result = verify_identity(
            &identity_public_key,
            keys_server_url.as_ref(),
            &account.to_did_pkh(),
            None,
            Some(&MockGetRpcUrl),
            None,
        )
        .await;

        assert!(matches!(
            result,
            Err(IdentityVerificationError::Client(
                IdentityVerificationClientError::CacaoRecapAbilityEmptyObjects
            ))
        ));
    }

    // TODO do we want to enable this test?
    #[tokio::test]
    #[ignore]
    async fn recaps_fail_uri_not_match_domain() {
        let (account_signing_key, account) = generate_account();

        let keys_server = MockServer::start().await;
        let keys_server_url = keys_server.uri().parse::<Url>().unwrap();

        let (_identity_signing_key, identity_public_key) = generate_identity_key();

        let project_id = ProjectId::generate();
        let app_domain = DidWeb::from_domain(format!("{project_id}.walletconnect.com"));

        let cacao = {
            let did_key = identity_public_key.to_did_key();
            let mut resources = vec![keys_server_url.to_string()];
            let domain = app_domain.domain();
            let recap = build_recap_details_object(Some(app_domain.domain()));
            resources.push(encode_recaip_uri(&recap));
            let statement = build_statement(&recap);
            let aud = {
                let mut url = format!("https://different.{domain}")
                    .parse::<Url>()
                    .unwrap();
                url.query_pairs_mut().append_pair(
                    cacao::payload::Payload::WALLETCONNECT_IDENTITY_KEY,
                    &did_key,
                );
                url.to_string()
            };
            let mut cacao = cacao::Cacao {
                h: cacao::header::Header {
                    t: EIP4361.to_owned().into(),
                },
                p: cacao::payload::Payload {
                    domain: app_domain.domain().to_owned(),
                    iss: account.to_did_pkh(),
                    statement: Some(statement),
                    aud,
                    version: cacao::Version::V1,
                    nonce: hex::encode(rand::Rng::gen::<[u8; 10]>(&mut rand::thread_rng())),
                    iat: Utc::now().to_rfc3339(),
                    exp: None,
                    nbf: None,
                    request_id: None,
                    resources: Some(resources),
                },
                s: cacao::signature::Signature {
                    t: "".to_owned(),
                    s: "".to_owned(),
                },
            };
            let (signature, recovery): (k256::ecdsa::Signature, _) = account_signing_key
                .sign_digest_recoverable(Keccak256::new_with_prefix(eip191_bytes(
                    &cacao.siwe_message().unwrap(),
                )))
                .unwrap();
            let cacao_signature = [&signature.to_bytes()[..], &[recovery.to_byte()]].concat();
            cacao.s.t = EIP191.to_owned();
            cacao.s.s = hex::encode(cacao_signature);
            cacao.verify(Some(&MockGetRpcUrl)).await.unwrap();
            cacao
        };

        register_mocked_identity_key(&keys_server, identity_public_key.clone(), cacao.clone())
            .await;

        let result = verify_identity(
            &identity_public_key,
            keys_server_url.as_ref(),
            &account.to_did_pkh(),
            None,
            Some(&MockGetRpcUrl),
            None,
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn recaps_fail_invalid_ability_domain() {
        let (account_signing_key, account) = generate_account();

        let keys_server = MockServer::start().await;
        let keys_server_url = keys_server.uri().parse::<Url>().unwrap();

        let (_identity_signing_key, identity_public_key) = generate_identity_key();

        let project_id = ProjectId::generate();
        let app_domain = DidWeb::from_domain(format!("{project_id}.walletconnect.com"));

        let cacao = {
            let did_key = identity_public_key.to_did_key();
            let mut resources = vec![keys_server_url.to_string()];
            let domain = app_domain.domain();
            let recap = build_recap_details_object(Some(&format!("*{}", app_domain.domain())));
            resources.push(encode_recaip_uri(&recap));
            let statement = build_statement(&recap);
            let aud = {
                let mut url = format!("https://{domain}").parse::<Url>().unwrap();
                url.query_pairs_mut().append_pair(
                    cacao::payload::Payload::WALLETCONNECT_IDENTITY_KEY,
                    &did_key,
                );
                url.to_string()
            };
            let mut cacao = cacao::Cacao {
                h: cacao::header::Header {
                    t: EIP4361.to_owned().into(),
                },
                p: cacao::payload::Payload {
                    domain: app_domain.domain().to_owned(),
                    iss: account.to_did_pkh(),
                    statement: Some(statement),
                    aud,
                    version: cacao::Version::V1,
                    nonce: hex::encode(rand::Rng::gen::<[u8; 10]>(&mut rand::thread_rng())),
                    iat: Utc::now().to_rfc3339(),
                    exp: None,
                    nbf: None,
                    request_id: None,
                    resources: Some(resources),
                },
                s: cacao::signature::Signature {
                    t: "".to_owned(),
                    s: "".to_owned(),
                },
            };
            let (signature, recovery): (k256::ecdsa::Signature, _) = account_signing_key
                .sign_digest_recoverable(Keccak256::new_with_prefix(eip191_bytes(
                    &cacao.siwe_message().unwrap(),
                )))
                .unwrap();
            let cacao_signature = [&signature.to_bytes()[..], &[recovery.to_byte()]].concat();
            cacao.s.t = EIP191.to_owned();
            cacao.s.s = hex::encode(cacao_signature);
            cacao.verify(Some(&MockGetRpcUrl)).await.unwrap();
            cacao
        };

        register_mocked_identity_key(&keys_server, identity_public_key.clone(), cacao.clone())
            .await;

        let result = verify_identity(
            &identity_public_key,
            keys_server_url.as_ref(),
            &account.to_did_pkh(),
            None,
            Some(&MockGetRpcUrl),
            None,
        )
        .await;
        assert!(matches!(
            result,
            Err(IdentityVerificationError::Client(
                IdentityVerificationClientError::CacaoRecapAbilityNameNotValidDomain
            ))
        ));
    }
}
