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
