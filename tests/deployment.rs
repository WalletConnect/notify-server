use {
    crate::utils::{
        assert_successful_response,
        http_api::subscribe_topic,
        notify_relay_api::{
            accept_notify_message, accept_watch_subscriptions_changed, subscribe,
            watch_subscriptions,
        },
        unregister_identity_key, RelayClient,
    },
    notify_server::{
        auth::{
            test_utils::{generate_identity_key, sign_cacao, CacaoAuth, IdentityKeyDetails},
            CacaoValue, DidWeb, STATEMENT_THIS_DOMAIN,
        },
        model::types::eip155::test_utils::generate_account,
        rpc::decode_key,
        services::public_http_server::handlers::notify_v0::NotifyBody,
        types::Notification,
        utils::topic_from_key,
    },
    relay_rpc::domain::ProjectId,
    std::{collections::HashSet, env},
    url::Url,
    uuid::Uuid,
};

mod utils;

// These tests test full integration of the notify server with other services. It is intended to test prod and staging environments in CD, but is also used to test locally.
// To run these tests locally, initialize `.env` with the "LOCAL deployment tests configuration" and "LOCAL or PROD deployment tests configuration"
//   and run `just run` and `just test-integration`.
//   To simplify development, `just devloop` can be used instead which automatically runs `just run` and also includes all other tests.

// These tests run against the LOCAL environment by default, but this can be changed by specifying the `ENVIRONMENT` variable, for example `ENVIRONMENT=DEV just test-integration`.
// If testing against DEV or STAGING environments, additional variables must be set in `.env` titled "DEV or STAGING deployment tests configuration"

// The Notify Server URL is chosen automatically depending on the chosen environment.
// Depending on the Notify Server chosen:
// - The necessary relay will be used. All relays use prod registry so the same prod project ID can be used.
// - The necessary NOTIFY_*_PROJECT_ID and NOTIFY_*_PROJECT_SECRET variables will be read. STAGING Notify Server uses staging registry, so a different project ID and secret must be used.
//   - To support CD, NOTIFY_PROJECT_ID and NOTIFY_PROJECT_SECRET variables are accepted as fallbacks. However to ease local development different variable names are used primiarly to avoid needing to change `.env` depending on which environment is being tested.
// The staging keys server is always used, to avoid unnecessary load on prod server.

fn get_vars() -> Vars {
    let relay_project_id = env::var("PROJECT_ID").unwrap().into();
    let keys_server_url = "https://staging.keys.walletconnect.com".parse().unwrap();

    let notify_prod_project_id = || {
        env::var("NOTIFY_PROD_PROJECT_ID")
            .unwrap_or_else(|_| env::var("NOTIFY_PROJECT_ID").unwrap())
            .into()
    };
    let notify_prod_project_secret = || {
        env::var("NOTIFY_PROD_PROJECT_SECRET")
            .unwrap_or_else(|_| env::var("NOTIFY_PROJECT_SECRET").unwrap())
    };
    let notify_staging_project_id = || {
        env::var("NOTIFY_STAGING_PROJECT_ID")
            .unwrap_or_else(|_| env::var("NOTIFY_PROJECT_ID").unwrap())
            .into()
    };
    let notify_staging_project_secret = || {
        env::var("NOTIFY_STAGING_PROJECT_SECRET")
            .unwrap_or_else(|_| env::var("NOTIFY_PROJECT_SECRET").unwrap())
    };

    let env = std::env::var("ENVIRONMENT").unwrap_or_else(|_| "LOCAL".to_owned());
    match env.as_str() {
        "PROD" => Vars {
            notify_url: "https://notify.walletconnect.com".parse().unwrap(),
            relay_url: "https://relay.walletconnect.com".parse().unwrap(),
            relay_project_id,
            notify_project_id: notify_prod_project_id(),
            notify_project_secret: notify_prod_project_secret(),
            keys_server_url,
        },
        "STAGING" => Vars {
            notify_url: "https://staging.notify.walletconnect.com".parse().unwrap(),
            relay_url: "https://staging.relay.walletconnect.com".parse().unwrap(),
            relay_project_id,
            notify_project_id: notify_staging_project_id(),
            notify_project_secret: notify_staging_project_secret(),
            keys_server_url,
        },
        "DEV" => Vars {
            notify_url: "https://dev.notify.walletconnect.com".parse().unwrap(),
            relay_url: "https://staging.relay.walletconnect.com".parse().unwrap(),
            relay_project_id,
            notify_project_id: notify_prod_project_id(),
            notify_project_secret: notify_prod_project_secret(),
            keys_server_url,
        },
        "LOCAL" => Vars {
            notify_url: "http://127.0.0.1:3000".parse().unwrap(),
            relay_url: "http://127.0.0.1:8888".parse().unwrap(),
            relay_project_id,
            notify_project_id: notify_prod_project_id(),
            notify_project_secret: notify_prod_project_secret(),
            keys_server_url,
        },
        e => panic!("Invalid ENVIRONMENT: {}", e),
    }
}

struct Vars {
    notify_url: Url,
    relay_url: Url,
    relay_project_id: ProjectId,
    notify_project_id: ProjectId,
    notify_project_secret: String,
    keys_server_url: Url,
}

#[tokio::test]
async fn deployment_integration() {
    let vars = get_vars();

    let (account_signing_key, account) = generate_account();

    let (identity_signing_key, identity_public_key) = generate_identity_key();
    let identity_key_details = IdentityKeyDetails {
        keys_server_url: vars.keys_server_url,
        signing_key: identity_signing_key,
        client_id: identity_public_key.clone(),
    };

    let project_id = vars.notify_project_id;
    let app_domain = DidWeb::from_domain(format!("{project_id}.example.com"));
    let (key_agreement, authentication_key, app_client_id) = subscribe_topic(
        &project_id,
        &vars.notify_project_secret,
        app_domain.clone(),
        &vars.notify_url,
    )
    .await;

    assert_successful_response(
        reqwest::Client::builder()
            .build()
            .unwrap()
            .post(
                identity_key_details
                    .keys_server_url
                    .join("/identity")
                    .unwrap(),
            )
            .json(&CacaoValue {
                cacao: sign_cacao(
                    &app_domain,
                    &account,
                    CacaoAuth::Statement(STATEMENT_THIS_DOMAIN.to_owned()),
                    identity_public_key.clone(),
                    identity_key_details.keys_server_url.to_string(),
                    &account_signing_key,
                )
                .await,
            })
            .send()
            .await
            .unwrap(),
    )
    .await;

    let mut relay_client = RelayClient::new(
        vars.relay_url,
        vars.relay_project_id,
        vars.notify_url.clone(),
    )
    .await;

    let (subs, watch_topic_key, notify_server_client_id) = watch_subscriptions(
        &mut relay_client,
        vars.notify_url.clone(),
        &identity_key_details,
        Some(app_domain.clone()),
        &account,
    )
    .await
    .unwrap();
    assert!(subs.is_empty());

    let notification_type = Uuid::new_v4();
    let notification_types = HashSet::from([notification_type]);
    let mut relay_client2 = relay_client.clone();
    subscribe(
        &mut relay_client,
        &account,
        &identity_key_details,
        key_agreement,
        &app_client_id,
        app_domain.clone(),
        notification_types.clone(),
    )
    .await
    .unwrap();
    let subs = accept_watch_subscriptions_changed(
        &mut relay_client2,
        &notify_server_client_id,
        &identity_key_details,
        &account,
        watch_topic_key,
    )
    .await
    .unwrap();
    assert_eq!(subs.len(), 1);
    let sub = &subs[0];
    assert_eq!(sub.account, account);
    assert_eq!(sub.app_domain, app_domain.domain());

    let notify_key = decode_key(&sub.sym_key).unwrap();

    relay_client.subscribe(topic_from_key(&notify_key)).await;

    let notification = Notification {
        r#type: notification_type,
        title: "title".to_owned(),
        body: "body".to_owned(),
        icon: None,
        url: None,
        data: None,
    };

    let notify_body = NotifyBody {
        notification_id: None,
        notification: notification.clone(),
        accounts: vec![account.clone()],
    };

    assert_successful_response(
        reqwest::Client::new()
            .post(
                vars.notify_url
                    .join(&format!("{project_id}/notify"))
                    .unwrap(),
            )
            .bearer_auth(vars.notify_project_secret)
            .json(&notify_body)
            .send()
            .await
            .unwrap(),
    )
    .await;

    let (_, claims) = accept_notify_message(
        &mut relay_client,
        &account,
        &authentication_key,
        &app_client_id,
        &app_domain,
        &notify_key,
    )
    .await
    .unwrap();

    assert_eq!(claims.msg.r#type, notification_type);
    assert_eq!(claims.msg.title, "title");
    assert_eq!(claims.msg.body, "body");
    assert_eq!(claims.msg.icon, "");
    assert_eq!(claims.msg.url, "");
    assert_eq!(claims.msg.data, None);

    unregister_identity_key(
        identity_key_details.keys_server_url,
        &account,
        &identity_key_details.signing_key,
        &identity_public_key,
    )
    .await;
}
