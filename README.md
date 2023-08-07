# Notify Server


[Notify Server Specs](https://docs.walletconnect.com/2.0/specs/servers/notify/notify-server-api)

[Current documentation](https://docs.walletconnect.com/2.0/specs/servers/notify/notify-server-api)



## Development

### Devloop

```bash
just amigood
```

### Integration tests

```bash
cp .env.example .env
nano .env
```

Note: `source .env` is unnecessary because justfile uses `set dotenv-load`

```bash
just run-storage-docker amigood run
```

```bash
just test-integration
```

```bash
just stop-storage-docker
```
