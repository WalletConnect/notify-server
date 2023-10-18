# Secrets

How to set GitHub Actions secrets.

## Global

TF_API_TOKEN

KEYPAIR_SEED
Set to a securly random value.

PROJECT_ID
Project ID for Notify Server to connect to relay. Should have rate limits disabled.
https://cloud.walletconnect.com/app/project?uuid=5f423bdd-12b2-4544-af6c-8a6ad470e7de

REGISTRY_AUTH_TOKEN
Registry auth token for prod Notify Server to authenticate project IDs and project secrets for dapps. Get from 1Password.

STAGING_REGISTRY_AUTH_TOKEN
Registry auth token for staging Notify Server to authenticate project IDs and project secrets for dapps. Get from 1Password.

## Both envs

VALIDATION_RELAY_PROJECT_ID
https://cloud.walletconnect.com/app/project?uuid=c2c31233-3630-4d7a-8649-653faeafe898

## `staging` env

VALIDATION_TEST_PROJECT_ID
VALIDATION_NOTIFY_PROJECT_SECRET
https://wc-cloud-staging.vercel.app/app/project?uuid=480ef7cc-a55a-451a-b76a-5f12ea28e077

VALIDATION_SWIFT_PROJECT_ID
https://cloud.walletconnect.com/app/project?uuid=fa897f4c-83a0-4f50-bd6b-53a9d94fce63

VALIDATION_SWIFT_DAPP_PROJECT_ID
VALIDATION_SWIFT_DAPP_PROJECT_SECRET
https://wc-cloud-staging.vercel.app/app/project?uuid=317a4b59-f0db-42e9-bffa-b32caf5f7ddd

## `prod` env

VALIDATION_TEST_PROJECT_ID
VALIDATION_NOTIFY_PROJECT_SECRET
https://cloud.walletconnect.com/app/project?uuid=c2c31233-3630-4d7a-8649-653faeafe898

VALIDATION_SWIFT_PROJECT_ID
https://cloud.walletconnect.com/app/project?uuid=fa897f4c-83a0-4f50-bd6b-53a9d94fce63

VALIDATION_SWIFT_DAPP_PROJECT_ID
VALIDATION_SWIFT_DAPP_PROJECT_SECRET
https://cloud.walletconnect.com/app/project?uuid=ec020ad1-89bc-4f0f-b7bc-5602990e79b5
