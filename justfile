binary-crate            := "."
set dotenv-load

export JUST_ROOT        := justfile_directory()

# TODO: Disabled for now because origional run target has `ANSI_LOGS=true`. Not sure why
# # Build service for development
# build:
#   @echo '==> Building project'
#   cargo build

# # Run the service
# run: build
#   @echo '==> Running project (ctrl+c to exit)'
#   cargo run

run:
    @echo '==> Running notify server'
    ANSI_LOGS=true cargo run

# Fast check project for errors
check:
  @echo '==> Checking project for compile errors'
  cargo check

# Run project test suite, skipping storage tests
test:
  @echo '==> Testing project (default)'
  cargo test --lib --bins

# Run project test suite, including storage tests (requires storage docker services to be running)
test-all:
  @echo '==> Testing project (all features)'
  cargo test --all-features --lib --bins

# Clean build artifacts
clean:
  @echo '==> Cleaning project target/*'
  cargo clean

# Lint the project for any quality issues
lint: check fmt clippy commit-check

amigood: lint test test-all

# Run project linter
clippy:
  #!/bin/bash
  set -euo pipefail

  if command -v cargo-clippy >/dev/null; then
    echo '==> Running clippy'
    cargo clippy --all-features --tests -- -D clippy::all
  else
    echo '==> clippy not found in PATH, skipping'
  fi

# Run code formatting check
fmt:
  #!/bin/bash
  set -euo pipefail

  if command -v cargo-fmt >/dev/null; then
    echo '==> Running rustfmt'
    cargo +nightly fmt
  else
    echo '==> rustfmt not found in PATH, skipping'
  fi

  if command -v terraform -version >/dev/null; then
    echo '==> Running terraform fmt'
    terraform -chdir=terraform fmt -recursive
  else
    echo '==> terraform not found in PATH, skipping'
  fi

# Run commit checker
commit-check:
  #!/bin/bash
  set -euo pipefail

  if command -v cog >/dev/null; then
    echo '==> Running cog check'
    cog check --from-latest-tag
  else
    echo '==> cog not found in PATH, skipping'
  fi

test-integration ENV:
    @echo '==> Running integration tests'
    ENVIRONMENT="{{ENV}}" cargo test --test integration -- --nocapture

deploy-terraform ENV:
    @echo '==> Deploying terraform on env {{ENV}}'
    terraform -chdir=terraform workspace select {{ENV}}
    terraform -chdir=terraform apply --var-file=vars/{{ENV}}.tfvars

commit MSG:
    @echo '==> Committing changes'
    cargo +nightly fmt && \
    git commit -a -S -m "{{MSG}}"

tarp ENV:
    @echo '==> Checking test coverage'
    ENVIRONMENT={{ENV}} cargo tarpaulin
