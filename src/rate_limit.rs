use {
    crate::{
        error::{Error, Result},
        registry::storage::redis::Redis,
    },
    chrono::{Duration, Utc},
    redis::Script,
    std::{collections::HashMap, sync::Arc},
};

pub async fn token_bucket(
    redis: &Arc<Redis>,
    key: String,
    max_tokens: u32,
    interval: Duration,
    refill_rate: u32,
) -> Result<()> {
    let result =
        token_bucket_many(redis, vec![key.clone()], max_tokens, interval, refill_rate).await?;
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
) -> Result<HashMap<String, (i64, u64)>> {
    // Remaining is number of tokens remaining. -1 for rate limited.
    // Reset is the time at which there will be 1 more token than before. This could, for example, be used to cache a 0 token count.
    Script::new(TOKEN_BUCKET_SCRIPT)
        .key(keys)
        .arg(max_tokens)
        .arg(interval.num_milliseconds())
        .arg(refill_rate)
        .arg(Utc::now().timestamp_millis())
        .invoke_async::<_, String>(&mut redis.write_pool().get().await?)
        .await
        .map_err(Into::into)
        .map(|value| serde_json::from_str(&value).expect("Redis script should return valid JSON"))
}

// Adapted from https://github.com/upstash/ratelimit/blob/3a8cfb00e827188734ac347965cb743a75fcb98a/src/single.ts#L311
const TOKEN_BUCKET_SCRIPT: &str = r#"
local keys        = KEYS              -- identifier including prefixes
local maxTokens   = tonumber(ARGV[1]) -- maximum number of tokens
local interval    = tonumber(ARGV[2]) -- size of the window in milliseconds
local refillRate  = tonumber(ARGV[3]) -- how many tokens are refilled after each interval
local now         = tonumber(ARGV[4]) -- current timestamp in milliseconds

local results = {}

for i,key in ipairs(keys) do
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
    results[key] = {-1, refilledAt + interval}
  else
    local remaining = tokens - 1
    local expireAt = math.ceil(((maxTokens - remaining) / refillRate)) * interval

    redis.call("HSET", key, "refilledAt", refilledAt, "tokens", remaining)
    redis.call("PEXPIRE", key, expireAt)
    results[key] = {remaining, refilledAt + interval}
  end
end

-- Redis doesn't support Lua table responses: https://stackoverflow.com/a/24302613
return cjson.encode(results)
"#;
