# Secrets

How to set GitHub Actions secrets.

## Global

TF_API_TOKEN

## Both envs

PROJECT_ID
Project ID for tests to connect to relay.
https://cloud.walletconnect.com/app/project?uuid=c2c31233-3630-4d7a-8649-653faeafe898

## `staging` env

VALIDATION_NOTIFY_PROJECT_ID
VALIDATION_NOTIFY_PROJECT_SECRET
https://wc-cloud-staging.vercel.app/app/project?uuid=480ef7cc-a55a-451a-b76a-5f12ea28e077

VALIDATION_SWIFT_PROJECT_ID
https://cloud.walletconnect.com/app/project?uuid=fa897f4c-83a0-4f50-bd6b-53a9d94fce63

VALIDATION_SWIFT_DAPP_PROJECT_ID
VALIDATION_SWIFT_DAPP_PROJECT_SECRET
https://wc-cloud-staging.vercel.app/app/project?uuid=317a4b59-f0db-42e9-bffa-b32caf5f7ddd

## `prod` env

VALIDATION_NOTIFY_PROJECT_ID
VALIDATION_NOTIFY_PROJECT_SECRET
https://cloud.walletconnect.com/app/project?uuid=c2c31233-3630-4d7a-8649-653faeafe898

VALIDATION_SWIFT_PROJECT_ID
https://cloud.walletconnect.com/app/project?uuid=fa897f4c-83a0-4f50-bd6b-53a9d94fce63

VALIDATION_SWIFT_DAPP_PROJECT_ID
VALIDATION_SWIFT_DAPP_PROJECT_SECRET
https://cloud.walletconnect.com/app/project?uuid=ec020ad1-89bc-4f0f-b7bc-5602990e79b5

# Terraform

project_id
Project ID for Notify Server to connect to relay. Should have rate limits disabled.
https://cloud.walletconnect.com/app/project?uuid=5f423bdd-12b2-4544-af6c-8a6ad470e7de

registry_api_endpoint & registry_api_auth_token
Registry auth token for Notify Server to authenticate project IDs and project secrets for dapps. Get from 1Password.
Staging Notify Server uses staging registry, all other envs use prod registry.

keypair_seed
Set to a securly random value.
