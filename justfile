binary-crate            := "."
set dotenv-load

export JUST_ROOT        := justfile_directory()

# Build service for development
build:
  @echo '==> Building project'
  cargo build

run:
  @echo '==> Running project (ctrl+c to exit)'
  ANSI_LOGS=true cargo run

# Fast check project for errors
check:
  @echo '==> Checking project for compile errors'
  RUST_BACKTRACE=1 cargo check

test := ""

# Run project test suite
test:
  @echo '==> Testing project (default)'
  RUST_BACKTRACE=1 cargo test --lib --bins -- {{test}}

# Run project test suite
test-all:
  @echo '==> Testing project (all features)'
  RUST_BACKTRACE=1 cargo test --all-features --lib --bins -- {{test}}

test-integration:
  @echo '==> Testing integration'
  RUST_BACKTRACE=1 ANSI_LOGS=true cargo test --test integration -- {{test}}

# Clean build artifacts
clean:
  @echo '==> Cleaning project target/*'
  cargo clean

# Lint the project for any quality issues
lint: clippy fmt

unit: lint test test-all lint-tf

devloop: unit fmt-imports
  #!/bin/bash -eux
  trap 'jobs -lp | xargs -L 1 pkill -SIGINT -P' EXIT

  pushd rs-relay
  just run-storage-docker build
  source .env
  just run &
  while ! nc -z 127.0.0.1 8888; do sleep 1; done
  popd

  just run-storage-docker test-integration

  # just run &
  # while ! nc -z 127.0.0.1 3000; do sleep 1; done
  # just test-deployment

  echo "✅ Success! ✅"

# Run project linter
clippy:
  #!/bin/bash
  set -euo pipefail

  if command -v cargo-clippy >/dev/null; then
    echo '==> Running clippy'
    cargo clippy --all-features --tests -- -D warnings
  else
    echo '==> clippy not found in PATH, skipping'
  fi

# Check unused depdenencies
udeps:
  #!/bin/bash
  set -euo pipefail

  if command -v cargo-udeps >/dev/null; then
    echo '==> Running udeps'
    cargo +nightly udeps
  else
    echo '==> udeps not found in PATH, skipping'
  fi

# Run code formatting check
fmt:
  #!/bin/bash
  set -euo pipefail

  if command -v cargo-fmt >/dev/null; then
    echo '==> Running rustfmt'
    cargo fmt
  else
    echo '==> rustfmt not found in PATH, skipping'
  fi

  if command -v terraform -version >/dev/null; then
    echo '==> Running terraform fmt'
    terraform -chdir=terraform fmt -recursive
  else
    echo '==> terraform not found in PATH, skipping'
  fi

fmt-imports:
  #!/bin/bash
  set -euo pipefail

  if command -v cargo-fmt >/dev/null; then
    echo '==> Running rustfmt'
    cargo +nightly fmt -- --config group_imports=StdExternalCrate,imports_granularity=One
  else
    echo '==> rustfmt not found in PATH, skipping'
  fi

lint-tf: tf-validate tf-fmt tfsec tflint tfdocs

# Check Terraform formating
tf-fmt:
  #!/bin/bash
  set -euo pipefail

  if command -v terraform >/dev/null; then
    echo '==> Running terraform fmt'
    terraform -chdir=terraform fmt -recursive
  else
    echo '==> Terraform not found in PATH, skipping'
  fi

tf-validate:
  #!/bin/bash
  set -euo pipefail

  if command -v terraform >/dev/null; then
    echo '==> Running terraform fmt'
    terraform -chdir=terraform validate
  else
    echo '==> Terraform not found in PATH, skipping'
  fi

# Check Terraform for potential security issues
tfsec:
  #!/bin/bash
  set -euo pipefail

  if command -v tfsec >/dev/null; then
    echo '==> Running tfsec'
    cd terraform
    tfsec
  else
    echo '==> tfsec not found in PATH, skipping'
  fi

# Run Terraform linter
tflint:
  #!/bin/bash
  set -euo pipefail

  if command -v tflint >/dev/null; then
    echo '==> Running tflint'
    cd terraform; tflint
    cd ecs; tflint
    cd ../monitoring; tflint
    cd ../private_zone; tflint
    cd ../redis; tflint

  else
    echo '==> tflint not found in PATH, skipping'
  fi

# Run terraform-docs
tfdocs:
  #!/bin/bash
  set -euo pipefail

  if command -v terraform-docs >/dev/null; then
    echo '==> Running terraform-docs'
    terraform-docs terraform
  else
    echo '==> terraform-docs not found in PATH, skipping'
  fi

test-deployment:
    @echo '==> Running deployment tests'
    RUST_BACKTRACE=1 cargo test --test deployment

test-deployment-nocapture:
    @echo '==> Running deployment tests'
    RUST_BACKTRACE=1 cargo test --test deployment -- --nocapture

deploy-terraform ENV:
    @echo '==> Deploying terraform on env {{ENV}}'
    terraform -chdir=terraform workspace select {{ENV}}
    terraform -chdir=terraform apply --var-file=vars/{{ENV}}.tfvars

tarp ENV:
    @echo '==> Checking test coverage'
    ENVIRONMENT={{ENV}} cargo tarpaulin

# Build docker image
build-docker:
  @echo '=> Build notify-server image'
  docker-compose -f ./docker-compose.notify-server.yml -f ./docker-compose.storage.yml build notify-server

# Start notify-server & storage services on docker
run-docker:
  @echo '==> Start services on docker'
  @echo '==> Use run notify-server app on docker with "cargo-watch"'
  @echo '==> for more details check https://crates.io/crates/cargo-watch'
  docker-compose -f ./docker-compose.notify-server.yml -f ./docker-compose.storage.yml up -d

# Stop notify-server & storage services on docker
stop-docker:
  @echo '==> Stop services on docker'
  docker-compose -f ./docker-compose.notify-server.yml -f ./docker-compose.storage.yml down --remove-orphans

# Start storage services on docker
run-storage-docker:
  @echo '==> Start storage services on docker'
  docker-compose -f ./docker-compose.storage.yml up -d --remove-orphans

# Stop gilgamesh & storage services on docker
stop-storage-docker:
  @echo '==> Stop storage services on docker'
  docker-compose -f ./docker-compose.storage.yml down --remove-orphans

# List services running on docker
ps-docker:
  @echo '==> List services on docker'
  docker-compose -f ./docker-compose.notify-server.yml -f ./docker-compose.storage.yml ps
