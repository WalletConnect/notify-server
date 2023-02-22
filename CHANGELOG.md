# Changelog
All notable changes to this project will be documented in this file. See [conventional commits](https://www.conventionalcommits.org/) for commit guidelines.

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
- added docsdb and cast to main.tf - (fc5f01e) - Rakowskiii
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