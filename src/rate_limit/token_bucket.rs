use {
    crate::{
        error::NotifyServerError,
        registry::storage::redis::Redis,
        services::public_http_server::handlers::relay_webhook::error::{
            RelayMessageClientError, RelayMessageError, RelayMessageServerError,
        },
    },
    chrono::{DateTime, Duration, Utc},
    core::fmt,
    deadpool_redis::PoolError,
    redis::{RedisError, Script},
    std::{collections::HashMap, sync::Arc},
};

pub trait ClockImpl: fmt::Debug + Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

pub type Clock = Option<Arc<dyn ClockImpl>>;

#[derive(Debug, thiserror::Error)]
#[error("Rate limit exceeded. Try again at {reset}")]
pub struct RateLimitExceeded {
    reset: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum RateLimitError {
    #[error(transparent)]
    RateLimitExceeded(RateLimitExceeded),

    #[error("Internal error: {0}")]
    InternalError(InternalRateLimitError),
}

impl From<RateLimitError> for NotifyServerError {
    fn from(error: RateLimitError) -> Self {
        match error {
            RateLimitError::RateLimitExceeded(e) => Self::TooManyRequests(e),
            RateLimitError::InternalError(e) => Self::RateLimitError(e),
        }
    }
}

impl From<RateLimitError> for RelayMessageError {
    fn from(error: RateLimitError) -> Self {
        match error {
            RateLimitError::RateLimitExceeded(e) => {
                RelayMessageClientError::RateLimitExceeded(e).into()
            }
            RateLimitError::InternalError(e) => {
                RelayMessageServerError::NotifyServerError(e.into()).into()
            }
        }
    }
}

pub async fn token_bucket(
    redis: &Arc<Redis>,
    key: String,
    max_tokens: u32,
    interval: Duration,
    refill_rate: u32,
    clock: &Clock,
) -> Result<(), RateLimitError> {
    let result = token_bucket_many(
        redis,
        vec![key.clone()],
        max_tokens,
        interval,
        refill_rate,
        clock,
    )
    .await
    .map_err(RateLimitError::InternalError)?;
    let (remaining, reset) = result.get(&key).expect("Should contain the key");
    if remaining.is_negative() {
        Err(RateLimitError::RateLimitExceeded(RateLimitExceeded {
            reset: reset / 1000,
        }))
    } else {
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InternalRateLimitError {
    #[error("Pool error {0}")]
    PoolError(PoolError),

    #[error("Redis error: {0}")]
    RedisError(RedisError),
}

pub async fn token_bucket_many(
    redis: &Arc<Redis>,
    keys: Vec<String>,
    max_tokens: u32,
    interval: Duration,
    refill_rate: u32,
    clock: &Clock,
) -> Result<HashMap<String, (i64, u64)>, InternalRateLimitError> {
    let now = clock.as_ref().map_or_else(Utc::now, |clock| clock.now());

    // Remaining is number of tokens remaining. -1 for rate limited.
    // Reset is the time at which there will be 1 more token than before. This could, for example, be used to cache a 0 token count.
    Script::new(include_str!("token_bucket.lua"))
        .key(keys)
        .arg(max_tokens)
        .arg(interval.num_milliseconds())
        .arg(refill_rate)
        .arg(now.timestamp_millis())
        .invoke_async::<_, String>(
            &mut redis
                .write_pool()
                .get()
                .await
                .map_err(InternalRateLimitError::PoolError)?,
        )
        .await
        .map_err(InternalRateLimitError::RedisError)
        .map(|value| serde_json::from_str(&value).expect("Redis script should return valid JSON"))
}
