use {
    crate::{
        error::{Error, Result},
        registry::storage::redis::Redis,
    },
    chrono::{DateTime, Duration, Utc},
    core::fmt,
    redis::Script,
    std::{collections::HashMap, sync::Arc},
};

pub trait ClockImpl: fmt::Debug + Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

pub type Clock = Option<Arc<dyn ClockImpl>>;

pub async fn token_bucket(
    redis: &Arc<Redis>,
    key: String,
    max_tokens: u32,
    interval: Duration,
    refill_rate: u32,
    clock: &Clock,
) -> Result<()> {
    let result = token_bucket_many(
        redis,
        vec![key.clone()],
        max_tokens,
        interval,
        refill_rate,
        clock,
    )
    .await?;
    let (remaining, reset) = result.get(&key).unwrap();
    if remaining.is_negative() {
        Err(Error::TooManyRequests(reset / 1000))
    } else {
        Ok(())
    }
}

pub async fn token_bucket_many(
    redis: &Arc<Redis>,
    keys: Vec<String>,
    max_tokens: u32,
    interval: Duration,
    refill_rate: u32,
    clock: &Clock,
) -> Result<HashMap<String, (i64, u64)>> {
    let now = clock.as_ref().map_or_else(Utc::now, |clock| clock.now());

    // Remaining is number of tokens remaining. -1 for rate limited.
    // Reset is the time at which there will be 1 more token than before. This could, for example, be used to cache a 0 token count.
    Script::new(include_str!("token_bucket.lua"))
        .key(keys)
        .arg(max_tokens)
        .arg(interval.num_milliseconds())
        .arg(refill_rate)
        .arg(now.timestamp_millis())
        .invoke_async::<_, String>(&mut redis.write_pool().get().await?)
        .await
        .map_err(Into::into)
        .map(|value| serde_json::from_str(&value).expect("Redis script should return valid JSON"))
}
