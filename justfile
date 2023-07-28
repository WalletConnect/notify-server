binary-crate            := "."
set dotenv-load 

export JUST_ROOT        := justfile_directory()

run: 
    @echo '==> Running notify server'
    ANSI_LOGS=true cargo run 

test ENV: 
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
