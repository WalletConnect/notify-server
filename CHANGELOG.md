# Changelog
All notable changes to this project will be documented in this file. See [conventional commits](https://www.conventionalcommits.org/) for commit guidelines.

- - -
## v0.15.0 - 2023-10-17
#### Bug Fixes
- test-threads in CI - (fe3c2e8) - Chris Smith
- notify key migration & add tests - (624c0f0) - Chris Smith
- NotifyMessage data export - (b42df6f) - Chris Smith
- remaining NotifyClient data - (c97abba) - Chris Smith
- filter scope in /notify - (caa1fc6) - Chris Smith
- cannot use CTE with modification statements as they are unpredictable - (23ad484) - Chris Smith
- domain regex - (ccfb153) - Chris Smith
#### Features
- re-enable scopes - (cf8dddb) - Chris Smith
- auto-migrate projects - (c92a5c7) - Chris Smith
- projects in Postgres - (68d8e9f) - Chris Smith
- Postgres boilerplate - (3ee6dfc) - Chris Smith
#### Miscellaneous Chores
- fix imports - (130e5ea) - Chris Smith
- Merge branch 'main' of https://github.com/WalletConnect/notify-server into feat/postgres - (a5bf320) - Chris Smith
- Bump version for release - (6bc68c5) - xav
- migrate CI, AWS account and alerting (#125) - (fb5177b) - Xavier Basty
- test two projects - (e6b7c62) - Chris Smith
- test two subscribers - (c375642) - Chris Smith
- refactor - (d952986) - Chris Smith
- FIXME fixed - (674c1e8) - Chris Smith
- remove more MongoDB - (d4f5f1b) - Chris Smith
- redundant var - (421ba5d) - Chris Smith
- fmt query - (8b3f502) - Chris Smith
- remove comments - (4cffc97) - Chris Smith
- Merge branch 'main' of https://github.com/WalletConnect/notify-server into feat/postgres - (4c07148) - Chris Smith
- remove fixme - (df645e1) - Chris Smith
- spawn for handle_msg() to avoid blocking reconnect - (4965e38) - Chris Smith
- idempotency & updated_at - (37e8f50) - Chris Smith
- rename - (315b9e6) - Chris Smith
- remove docdb from state - (b95389a) - Chris Smith
- combine subscribes - (3494516) - Chris Smith
- major work - (2b39f88) - Chris Smith
- Merge branch 'main' of https://github.com/WalletConnect/notify-server into feat/postgres - (bba0edd) - Chris Smith
- link to issue - (e4fba04) - Chris Smith
- Merge branch 'main' of https://github.com/WalletConnect/notify-server into feat/postgres - (0d61bf1) - Chris Smith
- remove unused struct - (0922b94) - Chris Smith
- style - (78b1775) - Chris Smith
- style - (294f6cc) - Chris Smith
- fmt - (fb62ec0) - Chris Smith
- Terraform Postgres - (b682478) - Chris Smith
- CI Postgres - (923cf8f) - Chris Smith
- clippy warnings - (55b3658) - Chris Smith

- - -

## v0.14.2 - 2023-10-17
#### Bug Fixes
- Terraform workspace in CI - (2d43c97) - Xavier Basty
- restore CI - (e0ff107) - Xavier Basty
- switch tests to real domain - (d0b6f50) - Xavier Basty
- use vars for autoscaling settings - (8d61602) - Xavier Basty
#### Miscellaneous Chores
- migrate CI, AWS account and alerting - (c11137d) - Xavier Basty

- - -

## v0.14.0 - 2023-04-12
#### Bug Fixes
- allow actions to force push version bump - (c07bcf4) - Rakowskiii
- add content:write permission to workflow - (02cd98f) - Rakowskiii
- fixed secrets in workflows - (86d77cc) - Rakowskiii
- validate workflow uppercase - (26c4c2f) - Rakowskiii
- testing for proper logging - (1e3385b) - Rakowskiii
- change tracing to debug - (000def0) - Rakowskiii
- change log level - (54a10e9) - Rakowskiii
- missing protoc - (3392aa7) - Rakowskiii
- validate workflow - (a902719) - Rakowskiii
- unregister resubscribe (#18) - (c657391) - Rakowskiii
- remove unused token - (ca96b28) - Rakowskiii
#### Features
- add unregister service (#16) - (c0d5769) - Rakowskiii
- add proper tests (#15) - (9c8eab2) - Rakowskiii
#### Miscellaneous Chores
- debbuging workflow - (f873da8) - Rakowskiii
- update readme.md - (2934a98) - Rakowskiii
- - -

## v0.13.1 - 2023-02-22
#### Bug Fixes
- tests - (a7806fa) - Rakowskiii
- formatting - (aa369a9) - Rakowskiii
- fix naming from httpstarter - (e3e4c6b) - Rakowskiii
#### Miscellaneous Chores
- remove unwrap. fix name in grafana. - (9a01bcc) - Rakowskiii
- - -

## v0.13.0 - 2023-02-22
#### Features
- add more fields to grafana - (f44bfea) - Rakowskiii
#### Miscellaneous Chores
- cleanup - (e34d513) - Rakowskiii
- - -

## v0.12.1 - 2023-02-22
#### Bug Fixes
- fixed terraform - (2f6e7d4) - Rakowskiii
#### Miscellaneous Chores
- add error logs for errors - (0676eb0) - Rakowskiii
- - -

## v0.12.0 - 2023-02-22
#### Features
- add grafana dashboard - (cbb192c) - Rakowskiii
- - -

## v0.11.0 - 2023-02-14
#### Features
- add grafana - (7ba1f85) - Rakowskiii
- - -

## v0.10.1 - 2023-02-14
#### Bug Fixes
- changed error tuple into struct - (8b06a4d) - Rakowskiii
- - -

## v0.10.0 - 2023-02-13
#### Features
- added logging - (2dda1f4) - Rakowskiii
- - -

## v0.9.5 - 2023-02-13
#### Bug Fixes
- add random ids to jsonrpc - (909f219) - Rakowskiii
#### Miscellaneous Chores
- debugging jsonrpc id - (acb8ddb) - Rakowskiii
- - -

## v0.9.4 - 2023-02-13
#### Bug Fixes
- fixed inserts - (fb3cc23) - Rakowskiii
- - -

## v0.9.3 - 2023-02-13
#### Bug Fixes
- changed base64 in notification payload to padded - (5b0294f) - Rakowskiii
- - -

## v0.9.2 - 2023-02-10
#### Bug Fixes
- random id in jsonrpc - (dd9a530) - Rakowskiii
- - -

## v0.9.1 - 2023-02-10
#### Bug Fixes
- added porotc to docker - (cda0a65) - Rakowskiii
- - -

## v0.9.0 - 2023-02-10
#### Bug Fixes
- add protoc - (663b39d) - Rakowskiii
- add protoc - (8e1ebad) - Rakowskiii
#### Features
- monitoring - (97a327d) - Rakowskiii
- - -

## v0.8.2 - 2023-02-08
#### Bug Fixes
- formatting - (cd1c020) - Rakowskiii
- fix encryption and message payload - (a42a234) - Rakowskiii
- - -

## v0.8.1 - 2023-02-08
#### Bug Fixes
- added cors conf to allow content-type header - (3d30264) - Rakowskiii
- - -

## v0.8.0 - 2023-02-07
#### Features
- add cors - (f39728a) - Rakowskiii
- random nonce. Encryption with AEAD - (37ae988) - Rakowskiii
- - -

## v0.7.2 - 2023-02-07
#### Bug Fixes
- declared input in workflow - (4e16b0f) - Rakowskiii
- use proper image version - (6a3c454) - Rakowskiii
- fixing workflow - (494ebb0) - Rakowskiii
- - -

## v0.7.1 - 2023-02-07
#### Bug Fixes
- adding keypair_seed to workflow - (b13f9fe) - Rakowskiii
- - -

## v0.7.0 - 2023-02-06
#### Features
- changed projetid from header to path - (652fc15) - Rakowskiii
- - -

## v0.6.0 - 2023-02-06
#### Features
- filtering by project_id -> collection(project_id) (#11) - (f2dd465) - Rakowskiii
- - -

## v0.5.6 - 2023-02-06
#### Bug Fixes
- fixing docsdb tls problems - (208fdba) - Rakowskiii
- - -

## v0.5.5 - 2023-02-06
#### Bug Fixes
- fixed issues with tls - (003bbde) - Rakowskiii
- - -

## v0.5.4 - 2023-02-05
#### Bug Fixes
- added permissions to get-version - (07a995f) - Rakowskiii
- - -

## v0.5.3 - 2023-02-05
#### Bug Fixes
- commented out if from cd.yml - (0c9f6fb) - Rakowskiii
- - -

## v0.5.2 - 2023-02-05
#### Bug Fixes
- vars tf formatting - (4ce8e7e) - Rakowskiii
- main.tf formatting - (6e4b8fb) - Rakowskiii
- use proper image version - (9bcf381) - Rakowskiii
- fixing cd workflow - (a2d87cc) - Rakowskiii
#### Miscellaneous Chores
- trigger workflow (#9) - (5377099) - Rakowskiii
- - -

## v0.5.1 - 2023-02-03
#### Bug Fixes
- fixing terraform - (e23df60) - Rakowskiii
- fixing terraform files - (baa2b1b) - Rakowskiii
- - -

## v0.5.0 - 2023-02-03
#### Bug Fixes
- cd syntax - (89fb53d) - Derek
- cd syntax - (4f11f99) - Derek
- deployment version prefixed with v - (5027126) - Derek
#### Features
- retrigger ci/cd - (7c1ce89) - Derek
- comment in dpeendencies again - (ad53228) - Derek
- wrong build trigger - (4417515) - Derek
- more elegant pipelines - (f0173e0) - Derek
- - -

## v0.4.1 - 2023-02-03
#### Bug Fixes
- added terraform variable - (ff5bac1) - Rakowskiii
- - -

## v0.4.0 - 2023-02-03
#### Features
- added proper return for notify - (3aa89c3) - Rakowskiii
- - -

## v0.3.1 - 2023-02-03
#### Bug Fixes
- removed invalid check for kick-off-cd - (e52f5f5) - Rakowskiii
- - -

## v0.3.0 - 2023-02-03
#### Bug Fixes
- added workflow_call to release.yml - (1623027) - Rakowskiii
- triggering workflow - (6271531) - Rakowskiii
- fixing workflow indent - (025baa5) - Rakowskiii
- triggering workflow - (1d04a1b) - Rakowskiii
- fixing ci workflow - (c439c2a) - Rakowskiii
- removed dispatch from release workflow - (7289f30) - Rakowskiii
- fixed release workflow - (53efdc4) - Rakowskiii
- fixed the naming in deploy workflow - (095672f) - Rakowskiii
- added docsdb and notify to main.tf - (fc5f01e) - Rakowskiii
- fix tests - (97e7be9) - Rakowskiii
- env name in Terraform - (4c58948) - Xavier Basty-Kjellberg
- workflow name in `deploy-infra` - (bc64c5e) - Xavier Basty-Kjellberg
- secrets in CD - (2ecde4a) - Xavier Basty-Kjellberg
- use kms key `arn` instead of `id` (#3) - (ef8250c) - Xavier Basty
- remove `get-version` until there is a release (#2) - (79a789f) - Xavier Basty
#### Features
- triggering action - (0b555c9) - Rakowskiii
- added ecs terraform - (d03630e) - Rakowskiii
- added working docker-compose - (849e9dd) - Rakowskiii
- add infra deployment to prod (#4) - (342ffff) - Xavier Basty
- add `DocumentDB` cluster (#1) - (a7bdd54) - Xavier Basty
- Initial commit - (ca4b076) - Rakowskiii
#### Miscellaneous Chores
- **(version)** v0.2.0 - (d9d0d15) - github-actions[bot]
- small fixes (#8) - (fecbb49) - Rakowskiii
- Bump version for release - (5db7e4d) - arein
- remove postgress from workflow - (5854a18) - Rakowskiii
- Fully moved from mock data to user provided data - (a3a1837) - Rakowskiii
- - -

## v0.2.0 - 2023-02-03
#### Features
- triggering action - (0b555c9) - Rakowskiii
- - -

Changelog generated by [cocogitto](https://github.com/cocogitto/cocogitto).