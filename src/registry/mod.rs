use {
    crate::{error::Result, metrics::Metrics},
    hyper::header,
    relay_rpc::domain::ProjectId,
    serde::{Deserialize, Serialize},
    sha2::{Digest, Sha256},
    std::{
        sync::Arc,
        time::{Duration, Instant},
    },
    storage::{redis::Redis, KeyValueStorage},
    tracing::{error, warn},
    tungstenite::http::HeaderValue,
    url::Url,
};

pub mod extractor;
pub mod storage;

pub struct RegistryHttpClient {
    authentication_endpoint: Url,
    http_client: reqwest::Client,
    metrics: Option<Metrics>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryAuthResponse {
    pub is_valid: bool,
}

impl RegistryHttpClient {
    pub fn new(registry_url: Url, auth_token: &str, metrics: Option<Metrics>) -> Result<Self> {
        let authentication_endpoint =
            registry_url.join("/internal/project/validate-notify-keys")?;

        let mut auth_value = HeaderValue::from_str(&format!("Bearer {}", auth_token))?;

        // Make sure we're not leaking auth token in debug output.
        auth_value.set_sensitive(true);

        let mut headers = header::HeaderMap::new();
        headers.insert(header::AUTHORIZATION, auth_value);

        let http_client = reqwest::Client::builder()
            .default_headers(headers)
            .build()?;

        Ok(Self {
            authentication_endpoint,
            http_client,
            metrics,
        })
    }

    pub async fn authenticate(&self, id: &str, secret: &str) -> Result<hyper::StatusCode> {
        let start = Instant::now();
        let res = self
            .http_client
            .get(self.authentication_endpoint.clone())
            .query(&[("projectId", id), ("secret", secret)])
            .send()
            .await?;
        if let Some(metrics) = &self.metrics {
            metrics.registry_request(start);
        }

        if !res.status().is_success() {
            warn!(
                "non-success registry status: {}, body: {:?}",
                res.status(),
                res.text().await
            );
            return Ok(hyper::StatusCode::UNAUTHORIZED);
        }

        let res = res.json::<RegistryAuthResponse>().await?;
        let is_valid = res.is_valid;

        Ok(if is_valid {
            hyper::StatusCode::OK
        } else {
            hyper::StatusCode::UNAUTHORIZED
        })
    }
}

pub struct Registry {
    client: Arc<RegistryHttpClient>,
    cache: Option<Arc<Redis>>,
}

impl Registry {
    pub fn new(
        registry_url: Url,
        auth_token: &str,
        cache: Option<Arc<Redis>>,
        metrics: Option<Metrics>,
    ) -> Result<Self> {
        let client = Arc::new(RegistryHttpClient::new(registry_url, auth_token, metrics)?);
        Ok(Self { client, cache })
    }

    pub async fn is_authenticated(&self, project_id: ProjectId, secret: &str) -> Result<bool> {
        self.is_authenticated_internal(project_id, secret)
            .await
            .map_err(|e| {
                error!("Failed to authenticate project: {}", e);
                e
            })
    }

    async fn is_authenticated_internal(&self, project_id: ProjectId, secret: &str) -> Result<bool> {
        let mut hasher = Sha256::new();
        hasher.update(project_id.as_ref());
        hasher.update(secret);
        let hash = hasher.finalize();
        let hash = hex::encode(hash);

        if let Some(cache) = &self.cache {
            if let Some(validity) = cache.get(&hash).await? {
                return Ok(validity);
            }
        }

        let validity = self
            .client
            .authenticate(project_id.as_ref(), secret)
            .await?
            .is_success();

        if let Some(cache) = &self.cache {
            cache.set(&hash, &validity, Some(CACHE_TTL)).await?;
        }

        Ok(validity)
    }
}

const CACHE_TTL: Duration = Duration::from_secs(60 * 30);
