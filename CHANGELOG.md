# Changelog
All notable changes to this project will be documented in this file. See [conventional commits](https://www.conventionalcommits.org/) for commit guidelines.

- - -
## 0.13.1 - 2023-11-21
#### Bug Fixes
- optimize queries (#222) - (223f265) - Chris Smith

- - -

## 0.13.0 - 2023-11-17
#### Features
- Keys Server & Registry metrics (#219) - (d93c4a2) - Chris Smith

- - -

## 0.12.0 - 2023-11-17
#### Features
- Postgres metrics (#218) - (d7ed3ab) - Chris Smith

- - -

## 0.11.2 - 2023-11-17
#### Bug Fixes
- no 500 on app domain conflict (#215) - (7532c74) - Chris Smith
- allow empty scopes (#214) - (90b56b4) - Chris Smith

- - -

## 0.11.1 - 2023-11-17
#### Bug Fixes
- typo (#217) - (e0011b3) - Chris Smith
- alarm immediately (#213) - (2859fdc) - Chris Smith
- switch Grafana dashboard to UTC (#212) - (171b49d) - Chris Smith

- - -

## 0.11.0 - 2023-11-16
#### Features
- adding dead letter processing and publishing timeout (#208) - (c0eed90) - Max Kalashnikoff
#### Refactoring
- using only `update_message_processing_status` to change status (#201) - (864c6e9) - Max Kalashnikoff

- - -

## 0.10.0 - 2023-11-15
#### Features
- **(metrics)** adding messages queue stats (#200) - (bf24dca) - Max Kalashnikoff

- - -

## 0.9.0 - 2023-11-15
#### Features
- add LICENSE - (3fdf1b3) - Derek
#### Miscellaneous Chores
- error logs (#211) - (257aaab) - Chris Smith
- Revert "chore: style sql (#207)" - (c3e19f5) - Chris Smith
- style sql (#207) - (48d4319) - Chris Smith

- - -

## 0.8.0 - 2023-11-13
#### Features
- send notification ID in wc_notifyMessage (#205) - (9c6632c) - Chris Smith
#### Miscellaneous Chores
- log projects and did:keys early (#206) - (eb4270d) - Chris Smith
- prefer JSON RPC version constant (#204) - (a5ebffc) - Chris Smith

- - -

## 0.7.0 - 2023-11-13
#### Bug Fixes
- always in alarm state (#197) - (c0e2194) - Chris Smith
#### Features
- **(metrics)** adding publishing workers count and errors count (#192) - (281cadf) - Max Kalashnikoff

- - -

## 0.6.2 - 2023-11-10
#### Bug Fixes
- parallel subscriber notification insert (#193) - (cf62115) - Chris Smith
#### Miscellaneous Chores
- increase pg connections (#194) - (f2c16e4) - Chris Smith

- - -

## 0.6.1 - 2023-11-10
#### Bug Fixes
- metrics (#191) - (878f80f) - Chris Smith
#### Miscellaneous Chores
- configuration refactor & dev deployment (#190) - (629f1da) - Chris Smith

- - -

## 0.6.0 - 2023-11-10
#### Features
- v1 wip (#141) - (0b4e6af) - Chris Smith
#### Miscellaneous Chores
- dsiable cog check (#195) - (d0ebe05) - Chris Smith

- - -

## 0.5.0 - 2023-11-08
#### Features
- update statement (#182) - (5588f89) - Chris Smith
- enable deployments to dev account (#183) - (9b5f099) - Xavier Basty
#### Miscellaneous Chores
- remove linked issues check (#174) - (a35b10e) - Chris Smith

- - -

## 0.4.3 - 2023-11-06
#### Bug Fixes
- log JWT iss (#180) - (6397cd2) - Chris Smith

- - -

## 0.4.2 - 2023-11-03
#### Bug Fixes
- allow MPL-2.0 - (8aefc6a) - Chris Smith
#### Miscellaneous Chores
- update `utils` version (#177) - (94e72cc) - Xavier Basty
- optional URL and icon (#176) - (5bfbfa9) - Chris Smith
- add license check (#171) - (5b7a7b9) - Chris Smith
- remove deployment_window check (#173) - (0479057) - Chris Smith
- remove DocDB from infra (#169) - (365207c) - Xavier Basty

- - -

## 0.4.1 - 2023-11-01
#### Bug Fixes
- account not updated (#157) - (e883ec1) - Chris Smith

- - -

## 0.4.0 - 2023-11-01
#### Bug Fixes
- remove DocDB from runtime (#167) - (a9c64a0) - Chris Smith
- ECS rollout (#159) - (f77049e) - Chris Smith
#### Features
- fetch the list of OFAC blocked countries from GitHub variables (#163) - (1ec6be2) - Xavier Basty

- - -

## 0.3.1 - 2023-10-27
#### Bug Fixes
- did:key app_authentication_key (#153) - (d9fd96c) - Chris Smith
- watchSubscriptions for all domains not triggering subscriptionsChanged (#155) - (0a21011) - Chris Smith

- - -

## 0.3.0 - 2023-10-27
#### Features
- add appAuthenticationKey to NotifyWatchSubscription (#151) - (cbee355) - Chris Smith

- - -

## 0.2.4 - 2023-10-26
#### Bug Fixes
- parquet doesn't support Uuid exports - (2b4a481) - Chris Smith

- - -

## 0.2.3 - 2023-10-26
#### Bug Fixes
- data export refactors (#144) - (296d39e) - Chris Smith

- - -

## 0.2.2 - 2023-10-25
#### Bug Fixes
- robust migration (#147) - (48c8814) - Chris Smith

- - -

## 0.2.1 - 2023-10-24
#### Bug Fixes
- change publish to use http client in handlers (#142) - (f6bc5ac) - Max Kalashnikoff
- add checks write permission to CI (#138) - (46d7aa6) - Xavier Basty
- revert RDS to password based (#136) - (68a57a3) - Xavier Basty
#### Miscellaneous Chores
- switch Swift integration tests branch - (752f38a) - Chris Smith
- changing threshold for 5xx alerting rule (#140) - (9cdeb97) - Max Kalashnikoff
- refactor integration tests - (c60df00) - Chris Smith
- pass explorer-endpoint to Swift integration tests - (24d4782) - Chris Smith

- - -

## 0.2.0 - 2023-10-18
#### Bug Fixes
- maximum percent - (135c0df) - Chris Smith
- max 1 replica & ECS task version retrieval - (4ccc1af) - Chris Smith
- secret names - (07eacf9) - Chris Smith
#### Features
- switch to Postgres (#113) - (5fb0f54) - Chris Smith
#### Miscellaneous Chores
- document secrets (#132) - (f585bf5) - Chris Smith

- - -

## 0.1.0 - 2023-10-17
#### Bug Fixes
- **(cd)** disable alert on math expression (#96) - (0283006) - Chris Smith
- **(ci)** missing secret param - (4860f7a) - Chris Smith
- **(ci)** secret names once again - (6deb266) - Chris Smith
- **(ci)** secret name - (0e368dc) - Chris Smith
- **(ci)** need staging registry API token - (01ac4b6) - Chris Smith
- **(hotfix)** Cast not subscribed when sending message (#94) - (886703c) - Derek
- **(o11y)** alarms not hooked up to OpsGenie properly - (dad3cee) - Derek
- **(o11y)** alarms not configured properly (#27) - (f22e29f) - Derek
- only release on master branch & fix bumping of Cargo.lock version (#130) - (34ba191) - Chris Smith
- handle relay events in parallel (#127) - (22a7137) - Chris Smith
- apply geoblock at beginning of chain (#121) - (3e8ad15) - Xavier Basty
- watch subscriptions duration (#114) - (d146286) - Chris Smith
- secret name - (820dc8d) - Chris Smith
- use inputs directly - (d326101) - Chris Smith
- re-enable Swift integration tests (#111) - (1e6ec6e) - Chris Smith
- no special case sub (#109) - (2ce231a) - Chris Smith
- retry publishing (#107) - (7d89fcc) - Chris Smith
- race condition - (c9019db) - Chris Smith
- staging relay still uses prod registry - (94d94b7) - Chris Smith
- staging server keeps getting rolled back - (d39d8fa) - Chris Smith
- GitHub workflow syntax - (84ae822) - Chris Smith
- need checkout - (c069bc8) - Chris Smith
- follow SIWE spec (#98) - (ce3c198) - Chris Smith
- ksu only used for client authentication (#94) - (8c10565) - Chris Smith
- refId (#89) - (81cce1e) - Chris Smith
- alarm on percent of 4xx instead of count (#80) - (43eda25) - Chris Smith
- changing topic didn't work (#88) - (5e5640e) - Chris Smith
- catch publish error (#82) - (fb97dc9) - Chris Smith
- assert one topic per account (#73) - (487ab84) - Chris Smith
- adjust location of 4050 check to reduce test flakes (#78) - (b12c4be) - Chris Smith
- verify CACAO resources (#72) - (6ae0d40) - Chris Smith
- DocumentDB available memory alarm (#69) - (c6b3343) - Chris Smith
- alert refId (#66) - (8426177) - Chris Smith
- subscribed metrics (#64) - (76aca46) - Chris Smith
- tflint & auto-format (#58) - (7650178) - Chris Smith
- notify subscribe tag in integration tests (#56) - (b6972f8) - Chris Smith
- check conventional commits in CI (#54) - (56a164f) - Chris Smith
- grafana (#52) - (8e3c3ba) - Chris Smith
- remove pushSubscribe tag (#42) - (4ce6957) - Chris Smith
- tagging - (546c3ba) - Chris Smith
- swift validation (#47) - (b7ec7e4) - Chris Smith
- grafana dashboard (#31) - (ea46ab6) - Chris Smith
- use new registry API endpoint for validating project secret (#44) - (ffd3a6a) - Chris Smith
- jwt formats (#11) - (916d4a1) - Chris Smith
- subscribe request and response tags, add TODOs (#6) - (30aa770) - Chris Smith
- release PAT (#9) - (462499f) - Chris Smith
- branch name (#4) - (5927345) - Chris Smith
- unknown feature `proc_macro_span_shrink` - (c16dac7) - Chris Smith
- missing matrix job params - (1dd8d04) - Chris Smith
- indent - (a854e47) - Chris Smith
- typo - (a766edf) - Chris Smith
- "Failed to decrypt" warning - (e8f7461) - Chris Smith
- tests for new payload - (a8e021d) - Rakowskiii
- changed payload( broken tests) - (0408678) - Rakowskiii
- broken workflow - (16dd45b) - Rakowskiii
- push -> notify - (d3d28f3) - Rakowskiii
- change the iss field from Notify pubkey to dapp pubkey - (57bdb28) - Rakowskiii
- wc_push -> wc_notify - (faf1c3d) - Rakowskiii
- more test coverage - (139aca8) - Rakowskiii
- expiry on lookup data - (f3a8d79) - Rakowskiii
- test_project_id - (e004af0) - Rakowskiii
- add did:key - (126719f) - Rakowskiii
- use proper data for notify pubkey - (2542682) - Rakowskiii
- remove redundant call of swift tests - (7975702) - Rakowskiii
- key generation - (0d699e5) - Rakowskiii
- registry auth endpoint - (2347008) - Rakowskiii
- add justfile - (4aa9fbf) - Rakowskiii
- comment out failing dashboard - (595dd3f) - Rakowskiii
- get rid of 50year TTL, it's not required anymore - (b239b06) - Rakowskiii
- actions; remove unnecessary config - (2009be6) - Rakowskiii
- remove register handler - (552fd8b) - Rakowskiii
- remove redundant TEST_KEYPAIR_SEED - (e87b5fe) - Rakowskiii
- redundant project_secret - (4d0d873) - Rakowskiii
- terraform fmt - (2f94e02) - Rakowskiii
- redis pool size number -> string - (fb97579) - Rakowskiii
- redis pool size number -> string - (a7b5005) - Rakowskiii
- redis pool size in ecs - (60cf960) - Rakowskiii
- redis pool size - (3cc17fb) - Rakowskiii
- add registry auth token - (99892e5) - Rakowskiii
- add registry url - (2a9118a) - Rakowskiii
- temporary disable validate swift requirement - (ac6fc45) - Rakowskiii
- test permissions in workflow - (ddff813) - Rakowskiii
- test permissions in workflow - (a36ac33) - Rakowskiii
- default to main swift branch - (dd7cf2a) - Rakowskiii
- validate swift requires deploy - (8c686ed) - Rakowskiii
- recursion limit actions - (71407c7) - Rakowskiii
- swift validate workflow - (e783950) - Rakowskiii
- lookup data not being replaced properly (#103) - (c4251f1) - Rakowskiii
- remove redundant jsonrpc code (#101) - (9c8dfa8) - Rakowskiii
- workflows - (b95b154) - Rakowskiii
- misspel - (17f3dab) - Rakowskiii
- workflow missing .yml - (84acf83) - Rakowskiii
- tests and noop message; log msg id; fix ci (#96) - (528c1a8) - Rakowskiii
- update validate_swift.yml (#97) - (36d6d6e) - Bartosz Rozwarski
- add noop message to relay to triger extension of topic (#93) - (3cd9ff2) - Rakowskiii
- remove unnecessary build conf (#91) - (043afac) - Rakowskiii
- change ttl to match the spec (#87) - (d756756) - Rakowskiii
- recconnect to relay after error (#77) - (de2e1bc) - Rakowskiii
- more informative logs (#69) - (e7712a3) - Rakowskiii
- use both project_id and project_secret to generate pubkey (#67) - (93ee5f2) - Rakowskiii
- resubscribe to topics after recconnect (#64) - (655328a) - Rakowskiii
- workflow - (4cce98b) - Rakowskiii
- update validate swift (#61) - (4e44e73) - Bartosz Rozwarski
- fixup - (470712f) - Rakowskiii
- add lld and llvm (#60) - (f946621) - Rakowskiii
- remove redundant project actions (#52) - (71386fc) - Xavier Basty
- more logs for /notify calls (#43) - (15309ac) - Rakowskiii
- use right tag for `Application` (#31) - (a8569ca) - Xavier Basty
- downscale docdb (#34) - (8da1e19) - Rakowskiii
- downscale docdb for staging - (cd5bf54) - Rakowskiii
- change default type to subscribe all subscriptions (#30) - (17524e3) - Rakowskiii
- rename webhook register body to match specs - (2745adb) - Rakowskiii
- wrong method on register_webhook - (ad15b1d) - Rakowskiii
- missing env var in integration tests (#28) - (40969e0) - Rakowskiii
- test misspel - (4544908) - Rakowskiii
- better log level - (1280fdf) - Rakowskiii
- add `application` tag to created resources. (#26) - (9ec6e37) - Xavier Basty
- test withouth commit version bump - (2a32f7a) - Rakowskiii
- unregister test naming - (79fb5b2) - Rakowskiii
- terraform fmt - (0e13f3b) - Rakowskiii
- set log level - (c03136d) - Rakowskiii
- missing relay_url in prod - (e44a248) - Rakowskiii
- terraform missing relay_url - (86ee6e9) - Rakowskiii
- terraform use project_id - (7a819c7) - Rakowskiii
- debugging workflow for release - (e0580e0) - Rakowskiii
- workflow with PAT - (d7b9279) - Rakowskiii
- permissions - (3197478) - Rakowskiii
- permissions - (281074c) - Rakowskiii
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
- tests - (a7806fa) - Rakowskiii
- formatting - (aa369a9) - Rakowskiii
- fix naming from httpstarter - (e3e4c6b) - Rakowskiii
- fixed terraform - (2f6e7d4) - Rakowskiii
- changed error tuple into struct - (8b06a4d) - Rakowskiii
- add random ids to jsonrpc - (909f219) - Rakowskiii
- fixed inserts - (fb3cc23) - Rakowskiii
- changed base64 in notification payload to padded - (5b0294f) - Rakowskiii
- random id in jsonrpc - (dd9a530) - Rakowskiii
- added porotc to docker - (cda0a65) - Rakowskiii
- add protoc - (663b39d) - Rakowskiii
- add protoc - (8e1ebad) - Rakowskiii
- formatting - (cd1c020) - Rakowskiii
- fix encryption and message payload - (a42a234) - Rakowskiii
- added cors conf to allow content-type header - (3d30264) - Rakowskiii
- declared input in workflow - (4e16b0f) - Rakowskiii
- use proper image version - (6a3c454) - Rakowskiii
- fixing workflow - (494ebb0) - Rakowskiii
- adding keypair_seed to workflow - (b13f9fe) - Rakowskiii
- fixing docsdb tls problems - (208fdba) - Rakowskiii
- fixed issues with tls - (003bbde) - Rakowskiii
- added permissions to get-version - (07a995f) - Rakowskiii
- commented out if from cd.yml - (0c9f6fb) - Rakowskiii
- vars tf formatting - (4ce8e7e) - Rakowskiii
- main.tf formatting - (6e4b8fb) - Rakowskiii
- use proper image version - (9bcf381) - Rakowskiii
- fixing cd workflow - (a2d87cc) - Rakowskiii
- fixing terraform - (e23df60) - Rakowskiii
- fixing terraform files - (baa2b1b) - Rakowskiii
- cd syntax - (89fb53d) - Derek
- cd syntax - (4f11f99) - Derek
- deployment version prefixed with v - (5027126) - Derek
- added terraform variable - (ff5bac1) - Rakowskiii
- removed invalid check for kick-off-cd - (e52f5f5) - Rakowskiii
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
- **(ci)** speed up CI - (182610c) - Derek
- noop - (18e03c1) - Chris Smith
- new statement & app claim validation (#123) - (f63782b) - Chris Smith
- updated siwe (#119) - (acece19) - Chris Smith
- geo-blocking, replace `gorgon` with `utils-rs` (#117) - (3e40be7) - Xavier Basty
- expire watchers (#103) - (d9ed493) - Chris Smith
- Merge protocol refactor to prod (#101) - (42d840a) - Chris Smith
- notify latency panel (#84) - (a290205) - Chris Smith
- subscribe latency panel (#76) - (09fc189) - Chris Smith
- verify keys server Cacao (#65) - (9251a77) - Chris Smith
- read replica & bump instance class (#62) - (fda92b5) - Chris Smith
- pull cast server changes (#33) - (4869284) - Chris Smith
- update msg TTLs & fix msg_id (#37) - (2c6eb23) - Chris Smith
- add proper dapp url - (fc3b84f) - Rakowskiii
- jwt authed messages - (11c6f92) - Rakowskiii
- renaming - (8f16ddf) - Rakowskiii
- project validation (#99) - (ded66ce) - Rakowskiii
- validate account (#88) - (e8d441e) - Rakowskiii
- parquet export (#85) - (42565f7) - Rakowskiii
- propagate tags to ECS tasks - (8c1b4e8) - Derek
- add ELB and DocDB panels, remove no-data alerts for 4XX and 5XX (#74) - (53696ef) - Xavier Basty
- add ECS panels and alerts (#73) - (7861ba6) - Xavier Basty
- convert Grafana dashboard to `jsonnet` (#72) - (918f939) - Xavier Basty
- 70 move casts notify from websockets to http (#71) - (4c3963c) - Rakowskiii
- add tf var cast url to workflow (#57) - (5fa4cc8) - Rakowskiii
- confirm ack with relay (#45) - (719f8f2) - Rakowskiii
- update to run on mac os (#54) - (1d90653) - Bartosz Rozwarski
- add validate swift workflow (#53) - (9e6f018) - Bartosz Rozwarski
- add project issues workflow, update project id (#41) - (e86a1a4) - Xavier Basty
- migrate to grafana 9 (#33) - (7f80fbd) - Rakowskiii
- notify server stage 1 changes (#25) - (30e3505) - Rakowskiii
- add unregister service (#16) - (c0d5769) - Rakowskiii
- add proper tests (#15) - (9c8eab2) - Rakowskiii
- add more fields to grafana - (f44bfea) - Rakowskiii
- add grafana dashboard - (cbb192c) - Rakowskiii
- add grafana - (7ba1f85) - Rakowskiii
- added logging - (2dda1f4) - Rakowskiii
- monitoring - (97a327d) - Rakowskiii
- add cors - (f39728a) - Rakowskiii
- random nonce. Encryption with AEAD - (37ae988) - Rakowskiii
- changed projetid from header to path - (652fc15) - Rakowskiii
- filtering by project_id -> collection(project_id) (#11) - (f2dd465) - Rakowskiii
- retrigger ci/cd - (7c1ce89) - Derek
- comment in dpeendencies again - (ad53228) - Derek
- wrong build trigger - (4417515) - Derek
- more elegant pipelines - (f0173e0) - Derek
- added proper return for notify - (3aa89c3) - Rakowskiii
- triggering action - (0b555c9) - Rakowskiii
- added ecs terraform - (d03630e) - Rakowskiii
- added working docker-compose - (849e9dd) - Rakowskiii
- add infra deployment to prod (#4) - (342ffff) - Xavier Basty
- add `DocumentDB` cluster (#1) - (a7bdd54) - Xavier Basty
#### Miscellaneous Chores
- **(version)** v0.14.0 - (c901d3f) - github-actions[bot]
- **(version)** v0.13.1 - (d4830c3) - github-actions[bot]
- **(version)** v0.13.0 - (9e78e3b) - github-actions[bot]
- **(version)** v0.12.1 - (8ab67e4) - github-actions[bot]
- **(version)** v0.12.0 - (45632bd) - github-actions[bot]
- **(version)** v0.11.0 - (7d02a59) - github-actions[bot]
- **(version)** v0.10.1 - (9a1381e) - github-actions[bot]
- **(version)** v0.10.0 - (6c51dea) - github-actions[bot]
- **(version)** v0.9.5 - (e1f5506) - github-actions[bot]
- **(version)** v0.9.4 - (e0209e6) - github-actions[bot]
- **(version)** v0.9.3 - (31627ae) - github-actions[bot]
- **(version)** v0.9.2 - (ecde699) - github-actions[bot]
- **(version)** v0.9.1 - (fa48bb9) - github-actions[bot]
- **(version)** v0.9.0 - (2c17beb) - github-actions[bot]
- **(version)** v0.8.2 - (fe2d125) - github-actions[bot]
- **(version)** v0.8.1 - (6870556) - github-actions[bot]
- **(version)** v0.8.0 - (b674c62) - github-actions[bot]
- **(version)** v0.7.2 - (ca09dca) - github-actions[bot]
- **(version)** v0.7.1 - (2c56e4e) - github-actions[bot]
- **(version)** v0.7.0 - (5d5f3a9) - github-actions[bot]
- **(version)** v0.6.0 - (234bb51) - github-actions[bot]
- **(version)** v0.5.6 - (d249ead) - github-actions[bot]
- **(version)** v0.5.5 - (803a2ee) - github-actions[bot]
- **(version)** v0.5.4 - (b57e5b3) - github-actions[bot]
- **(version)** v0.5.3 - (81063c4) - github-actions[bot]
- **(version)** v0.5.2 - (2a00993) - github-actions[bot]
- **(version)** v0.5.1 - (5bf614a) - github-actions[bot]
- **(version)** v0.5.0 - (cf884c5) - github-actions[bot]
- **(version)** v0.4.1 - (d58a4c4) - github-actions[bot]
- **(version)** v0.4.0 - (7ef04ec) - github-actions[bot]
- **(version)** v0.3.1 - (4bfea67) - github-actions[bot]
- **(version)** v0.3.0 - (5bebcfd) - github-actions[bot]
- **(version)** v0.2.0 - (d9d0d15) - github-actions[bot]
- Bump version for release - (6bc68c5) - xav
- migrate CI, AWS account and alerting (#125) - (fb5177b) - Xavier Basty
- spec constants (#60) - (86785b5) - Chris Smith
- delete response code (#23) - (cfcf3e0) - Chris Smith
- Update validate_swift.yml to use apple-silicon runner group (#12) - (9eac658) - Radek Novak
- spinup - (fce34c7) - Chris Smith
- improve devex - (ca4eab5) - Chris Smith
- get running locally w/ integration tests, improve devloop and CI, fmt & clippy - (2956dcf) - Chris Smith
- assert response status - (1499a11) - Chris Smith
- TODOs and comments - (97f330e) - Chris Smith
- simplify - (3135d98) - Chris Smith
- Bump version for release - (d6af5b0) - Rakowskiii
- debbuging workflow - (f873da8) - Rakowskiii
- update readme.md - (2934a98) - Rakowskiii
- Bump version for release - (8fd389f) - Rakowskiii
- remove unwrap. fix name in grafana. - (9a01bcc) - Rakowskiii
- Bump version for release - (07abd97) - Rakowskiii
- cleanup - (e34d513) - Rakowskiii
- Bump version for release - (ec4b662) - Rakowskiii
- add error logs for errors - (0676eb0) - Rakowskiii
- Bump version for release - (1f46c45) - Rakowskiii
- Bump version for release - (b25b508) - Rakowskiii
- Bump version for release - (da3c146) - Rakowskiii
- Bump version for release - (f48de23) - Rakowskiii
- Bump version for release - (ca0ab2e) - Rakowskiii
- debugging jsonrpc id - (acb8ddb) - Rakowskiii
- Bump version for release - (1790e4e) - Rakowskiii
- Bump version for release - (8a4e61d) - Rakowskiii
- Bump version for release - (88926dd) - Rakowskiii
- Bump version for release - (cd67731) - Rakowskiii
- Bump version for release - (82dcc88) - Rakowskiii
- Bump version for release - (4ddbdeb) - Rakowskiii
- Bump version for release - (8c65ff8) - Rakowskiii
- Bump version for release - (5af149d) - Rakowskiii
- Bump version for release - (e984d26) - Rakowskiii
- Bump version for release - (d65b4e5) - Rakowskiii
- Bump version for release - (89904f5) - Rakowskiii
- Bump version for release - (a136809) - Rakowskiii
- Bump version for release - (38805cc) - Rakowskiii
- Bump version for release - (9ebbd11) - Rakowskiii
- Bump version for release - (8bc788e) - Rakowskiii
- Bump version for release - (a5f3a4e) - Rakowskiii
- Bump version for release - (370b0a1) - Rakowskiii
- trigger workflow (#9) - (5377099) - Rakowskiii
- Bump version for release - (6baf11b) - Rakowskiii
- Bump version for release - (b1dbf18) - arein
- Bump version for release - (f0759c6) - Rakowskiii
- Bump version for release - (f555a38) - Rakowskiii
- Bump version for release - (b2f8680) - Rakowskiii
- Bump version for release - (83bde11) - Rakowskiii
- small fixes (#8) - (fecbb49) - Rakowskiii
- Bump version for release - (5db7e4d) - arein
- remove postgress from workflow - (5854a18) - Rakowskiii
- Fully moved from mock data to user provided data - (a3a1837) - Rakowskiii

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
