-- Adapted from https://github.com/upstash/ratelimit/blob/3a8cfb00e827188734ac347965cb743a75fcb98a/src/single.ts#L311

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
