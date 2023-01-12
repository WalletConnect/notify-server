# Cast Server


[Cast Server Specs](https://docs.walletconnect.com/2.0/specs/servers/cast/cast-server-api)
[Current documentation](https://docs.walletconnect.com/2.0/specs/servers/cast/cast-server-api)



## Running the app

* Build: `cargo build`
* Test: `cargo test`
* Run: `docker-compose-up`
* Integration test: `yarn install` (once) and then `yarn integration:local(dev/staging/prod)`



## Required Values to Change

- [x] `README.md`
  Replace this file with a Readme for your project
- [x] `Cargo.toml`
  Change package name and authors, then build the project to re-gen the `Cargo.lock`
- [x] `/terraform/main.tf`
  Change `app_name` to the repo's name
- [x] `/terraform/main.tf`
  Setup a new hosted zone in the [infra repository](https://github.com/WalletConnect/infra/blob/master/terraform/main.tf#L123)
- [ ] `/terraform/variables.tf`
  Change the default value of `public_url`
- [ ] `/terraform/backend.tf`
  Change the `key` for the `s3` backend to your project's name
- [ ] `/.github/workflows/cd.yml`
  Ensure any URLs for health checks and environments match the terraform files
- [ ] `/.github/workflows/intake.yml`
  This is specific to WalletConnect, feel free to remove or modify for your own needs
- [ ] `/.github/ISSUE_TEMPLATE/feature_request.yml`
  Change any references to Rust HTTP Starter\
- [ ] `/.github/workflows/release.yml`
  On line 95-97 there are references to the registry name on ECR/GHCR ensure you change this
- [x] `/.github/integration/integration.tests.ts`
  Update the URLs

### WalletConnect Specific

- [ ] `/.github/workflows/**/*.yml`
  Change the `runs-on` to the `ubuntu-runners` group

## GitHub Secrets
Required GitHub secrets for Actions to run successfully
- `AWS_ACCESS_KEY_ID`
- `AWS_SECRET_ACCESS_KEY`
- `PAT` Personal Access Token for Github to commit releases

### WalletConnect Specific
- `ASSIGN_TO_PROJECT_GITHUB_TOKEN`
