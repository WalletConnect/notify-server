# Notify Server


[Notify Server Specs](https://docs.walletconnect.com/2.0/specs/servers/notify/notify-server-api)

[Current documentation](https://docs.walletconnect.com/2.0/specs/servers/notify/notify-server-api)



## Running the app

* Build: `cargo build`
* Test: `PROJECT_ID="<project_id>" RELAY_URL="<relay_url>" cargo test --test functional`
* Run: `docker-compose-up`
* Integration test: `PROJECT_ID="<project_id>" TEST_ENV="STAGING(STAGING/PROD)" cargo test --test integration` 


