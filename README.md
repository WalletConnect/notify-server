# Notify Server


[Notify Server Specs](https://docs.walletconnect.com/2.0/specs/servers/notify/notify-server-api)

[Current documentation](https://docs.walletconnect.com/2.0/specs/servers/notify/notify-server-api)



## Development

### Devloop

Runs all tests, storage tests, and integration tests automatically.

```bash
just devloop
```

### Storage tests

```bash
just run-storage-docker test-storage
```

```bash
just stop-storage-docker
```

### Integration tests

```bash
cp .env.example .env
nano .env
```

Note: `source .env` is unnecessary because justfile uses `set dotenv-load`

```bash
just run-storage-docker unit run

# With storage tests
just unit run-storage-docker test-storage stop-storage-docker run-storage-docker run
```

```bash
just test-integration
```

```bash
just stop-storage-docker
```

## Terraform dev deployment with local backend

```bash
cp .env.terraform.example .env.terraform
nano .env.terraform
```

```bash
source .env.terraform
terraform login
nano terraform/terraform.tf # comment out `backend "remote"` block
git submodule update --init --recursive
terraform -chdir=terraform init
terraform -chdir=terraform workspace new dev
terraform -chdir=terraform workspace select dev
terraform -chdir=terraform apply -var-file="vars/$(terraform -chdir=terraform workspace show).tfvars"
```

### Deploying local code changes

```bash
source .env.terraform
./terraform/deploy-dev.sh
```

#### Remote building

If amd64 builds are too slow on your Mac (likely), consider using a remote builder on a linux/amd64 host:

```bash
docker buildx create --name=remote-amd64 --driver=docker-container ssh://<my-amd64-host>
BUILD_ARGS="--builder=remote-amd64 --load" ./terraform/deploy-dev.sh
```
