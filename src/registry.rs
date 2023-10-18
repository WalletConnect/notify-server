use {
    crate::{
        config::Configuration,
        error::Result,
        storage::{redis::Redis, KeyValueStorage},
    },
    hyper::header,
    relay_rpc::domain::ProjectId,
    serde::{Deserialize, Serialize},
    sha2::{Digest, Sha256},
    std::{sync::Arc, time::Duration},
    tracing::{error, warn},
    tungstenite::http::HeaderValue,
};

pub struct RegistryHttpClient {
    addr: String,
    http_client: reqwest::Client,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryAuthResponse {
    pub is_valid: bool,
}

impl RegistryHttpClient {
    pub fn new(base_url: impl Into<String>, auth_token: &str) -> Result<Self> {
        let mut auth_value = HeaderValue::from_str(&format!("Bearer {}", auth_token))?;

        // Make sure we're not leaking auth token in debug output.
        auth_value.set_sensitive(true);

        let mut headers = header::HeaderMap::new();
        headers.insert(header::AUTHORIZATION, auth_value);

        let http_client = reqwest::Client::builder()
            .default_headers(headers)
            .build()?;

        Ok(Self {
            addr: base_url.into(),
            http_client,
        })
    }

    pub async fn authenticate(&self, id: &str, secret: &str) -> Result<hyper::StatusCode> {
        let url = format!(
            "{}/internal/project/validate-notify-keys?projectId={id}&secret={secret}",
            self.addr
        );

        let res = self.http_client.get(url).send().await?;
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
    pub fn new(url: &str, auth_token: &str, config: &Configuration) -> Result<Self> {
        let client = Arc::new(RegistryHttpClient::new(url, auth_token)?);

        let cache = if let Some(redis_addr) = &config.auth_redis_addr() {
            Some(Arc::new(Redis::new(
                redis_addr,
                config.redis_pool_size as usize,
            )?))
        } else {
            None
        };
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
