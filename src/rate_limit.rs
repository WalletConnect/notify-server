use {
    crate::{
        error::{Error, Result},
        registry::storage::redis::Redis,
    },
    chrono::{Duration, Utc},
    redis::Script,
    std::sync::Arc,
};

pub async fn token_bucket(
    redis: &Arc<Redis>,
    key: String,
    max_tokens: u32,
    interval: Duration,
    refill_rate: u32,
) -> Result<()> {
    // Remaining is number of tokens remaining. -1 for rate limited.
    // Reset is the time at which there will be 1 more token than before. This could, for example, be used to cache a 0 token count.
    let (remaining, reset) = Script::new(TOKEN_BUCKET_SCRIPT)
        .key(key)
        .arg(max_tokens)
        .arg(interval.num_milliseconds())
        .arg(refill_rate)
        .arg(Utc::now().timestamp_millis())
        .invoke_async::<_, (i64, u64)>(&mut redis.write_pool().get().await?)
        .await?;

    if remaining >= 0 {
        Ok(())
    } else {
        Err(Error::TooManyRequests(reset / 1000))
    }
}

// Adapted from https://github.com/upstash/ratelimit/blob/3a8cfb00e827188734ac347965cb743a75fcb98a/src/single.ts#L311
const TOKEN_BUCKET_SCRIPT: &str = r#"
local key         = KEYS[1]           -- identifier including prefixes
local maxTokens   = tonumber(ARGV[1]) -- maximum number of tokens
local interval    = tonumber(ARGV[2]) -- size of the window in milliseconds
local refillRate  = tonumber(ARGV[3]) -- how many tokens are refilled after each interval
local now         = tonumber(ARGV[4]) -- current timestamp in milliseconds

local bucket = redis.call("HMGET", key, "refilledAt", "tokens")

local refilledAt
local tokens

if bucket[1] == false then
  refilledAt = now
  tokens = maxTokens
else
  refilledAt = tonumber(bucket[1])
  tokens = tonumber(bucket[2])
end

if now >= refilledAt + interval then
  local numRefills = math.floor((now - refilledAt) / interval)
  tokens = math.min(maxTokens, tokens + numRefills * refillRate)

  refilledAt = refilledAt + numRefills * interval
end

if tokens == 0 then
  return {-1, refilledAt + interval}
end

local remaining = tokens - 1
local expireAt = math.ceil(((maxTokens - remaining) / refillRate)) * interval

redis.call("HSET", key, "refilledAt", refilledAt, "tokens", remaining)
redis.call("PEXPIRE", key, expireAt)
return {remaining, refilledAt + interval}
"#;
