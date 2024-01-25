# Notify Server


[Notify Server Specs](https://docs.walletconnect.com/2.0/specs/servers/notify/notify-server-api)

[Current documentation](https://docs.walletconnect.com/2.0/specs/servers/notify/notify-server-api)



## Development

### Dependencies

- Rust
- just
- docker

```bash
git submodule update --init --recursive
```

### Running the relay locally

```bash
cd rs-relay
# update .env file
source .env
just run-storage-docker
cargo run
```

### Devloop

Runs all tests, integration tests, and deployment tests automatically.

```bash
just devloop
```

### Integration tests

```bash
just run-storage-docker test-integration
```

Run a specific test:

```bash
just test=test_one_project test-integration
```

```bash
just stop-storage-docker
```

### Deployment tests

```bash
cp .env.example .env
nano .env
```

Note: `source .env` is unnecessary because justfile uses `set dotenv-load`

```bash
just run-storage-docker unit run

# With integration tests
just unit run-storage-docker test-integration stop-storage-docker run-storage-docker run
```

```bash
just test-deployment
```

```bash
just stop-storage-docker
```

## Terraform dev deployment

Make sure you provide some secrets:

```bash
cp .env.terraform.example .env.terraform
nano .env.terraform
```

You may need to initialize submodules and Terraform:

```bash
git submodule update --init --recursive
terraform login
terraform -chdir=terraform init
```

To deploy

```bash
source .env.terraform
./terraform/deploy-dev.sh
```

If amd64 builds are too slow on your Mac (likely), consider using a remote builder on a linux/amd64 host:

```bash
docker buildx create --name=remote-amd64 --driver=docker-container ssh://<my-amd64-host>
BUILD_ARGS="--builder=remote-amd64 --load" ./terraform/deploy-dev.sh
```
