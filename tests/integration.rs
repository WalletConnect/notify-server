use {
    crate::utils::{
        decode_authentication_public_key, encode_auth, verify_jwt, UnregisterIdentityRequestAuth,
        JWT_LEEWAY,
    },
    async_trait::async_trait,
    base64::{engine::general_purpose::STANDARD as BASE64, Engine},
    chacha20poly1305::{aead::Aead, ChaCha20Poly1305, KeyInit},
    chrono::{Duration, TimeZone, Utc},
    data_encoding::BASE64URL,
    ed25519_dalek::SigningKey,
    hyper::StatusCode,
    notify_server::{
        auth::{
            add_ttl, encode_authentication_private_key, encode_authentication_public_key,
            encode_subscribe_private_key, encode_subscribe_public_key, from_jwt, CacaoValue,
            DidWeb, KeyServerResponse, NotifyServerSubscription, SharedClaims,
            SubscriptionDeleteRequestAuth, SubscriptionDeleteResponseAuth, SubscriptionRequestAuth,
            SubscriptionResponseAuth, SubscriptionUpdateRequestAuth,
            SubscriptionUpdateResponseAuth, WatchSubscriptionsChangedRequestAuth,
            WatchSubscriptionsRequestAuth, WatchSubscriptionsResponseAuth,
            KEYS_SERVER_IDENTITY_ENDPOINT, KEYS_SERVER_IDENTITY_ENDPOINT_PUBLIC_KEY_QUERY,
            KEYS_SERVER_STATUS_SUCCESS, STATEMENT_ALL_DOMAINS, STATEMENT_THIS_DOMAIN,
        },
        config::Configuration,
        jsonrpc::NotifyPayload,
        model::{
            helpers::{
                get_project_by_app_domain, get_project_by_project_id, get_project_by_topic,
                get_project_topics, get_subscriber_accounts_and_scopes_by_project_id,
                get_subscriber_accounts_by_project_id, get_subscriber_by_topic,
                get_subscriber_topics, get_subscribers_for_project_in,
                get_subscriptions_by_account, upsert_project, upsert_subscriber,
                SubscriberAccountAndScopes,
            },
            types::AccountId,
        },
        rate_limit,
        registry::{storage::redis::Redis, RegistryAuthResponse},
        services::{
            public_http_server::{
                handlers::{
                    did_json::{
                        DidJson, WC_NOTIFY_AUTHENTICATION_KEY_ID, WC_NOTIFY_SUBSCRIBE_KEY_ID,
                    },
                    notify_v0::NotifyBody,
                    notify_v1::{
                        self, notify_rate_limit, subscriber_rate_limit, subscriber_rate_limit_key,
                        NotifyBodyNotification,
                    },
                    subscribe_topic::{SubscribeTopicRequestBody, SubscribeTopicResponseBody},
                },
                DID_JSON_ENDPOINT,
            },
            publisher_service::helpers::{
                dead_letter_give_up_check, dead_letters_check,
                pick_subscriber_notification_for_processing, upsert_notification,
                upsert_subscriber_notifications, NotificationToProcess,
            },
            websocket_server::{
                decode_key, derive_key, relay_ws_client::RelayClientEvent, NotifyRequest,
                NotifyResponse, NotifyWatchSubscriptions, ResponseAuth,
            },
        },
        spec::{
            NOTIFY_DELETE_METHOD, NOTIFY_DELETE_RESPONSE_TAG, NOTIFY_DELETE_TAG, NOTIFY_DELETE_TTL,
            NOTIFY_MESSAGE_TAG, NOTIFY_NOOP_TAG, NOTIFY_SUBSCRIBE_METHOD,
            NOTIFY_SUBSCRIBE_RESPONSE_TAG, NOTIFY_SUBSCRIBE_TAG, NOTIFY_SUBSCRIBE_TTL,
            NOTIFY_SUBSCRIPTIONS_CHANGED_TAG, NOTIFY_UPDATE_METHOD, NOTIFY_UPDATE_RESPONSE_TAG,
            NOTIFY_UPDATE_TAG, NOTIFY_UPDATE_TTL, NOTIFY_WATCH_SUBSCRIPTIONS_ACT,
            NOTIFY_WATCH_SUBSCRIPTIONS_METHOD, NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_ACT,
            NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_TAG, NOTIFY_WATCH_SUBSCRIPTIONS_TAG,
            NOTIFY_WATCH_SUBSCRIPTIONS_TTL,
        },
        types::{Envelope, EnvelopeType0, EnvelopeType1, Notification},
    },
    rand::rngs::StdRng,
    rand_chacha::rand_core::OsRng,
    rand_core::SeedableRng,
    relay_client::websocket::{Client, PublishedMessage},
    relay_rpc::{
        auth::{
            cacao::{
                self,
                header::EIP4361,
                signature::{Eip191, EIP191},
                Cacao,
            },
            ed25519_dalek::Keypair,
        },
        domain::{DecodedClientId, ProjectId, Topic},
    },
    reqwest::Response,
    serde_json::{json, Value},
    sha2::{digest::generic_array::GenericArray, Digest},
    sha3::Keccak256,
    sqlx::{postgres::PgPoolOptions, PgPool, Postgres},
    std::{
        collections::HashSet,
        env,
        net::{IpAddr, Ipv4Addr, SocketAddr},
        sync::Arc,
    },
    test_context::{test_context, AsyncTestContext},
    tokio::{
        net::{TcpListener, ToSocketAddrs},
        sync::{broadcast, mpsc::UnboundedReceiver},
        time::error::Elapsed,
    },
    tracing_subscriber::fmt::format::FmtSpan,
    url::Url,
    utils::{create_client, generate_account},
    uuid::Uuid,
    wiremock::{
        http::Method,
        matchers::{method, path, query_param},
        Mock, MockServer, ResponseTemplate,
    },
    x25519_dalek::{PublicKey, StaticSecret},
};

mod utils;

// Unit-like integration tests able to be run locally with minimal configuration; only relay project ID is required.
// Simply initialize .env with the integration configuration and run `just test-integration`

// The only variable that's needed is a valid relay project ID because the relay is not mocked.
// The registry is mocked out, so any project ID or notify secret is valid and are generated randomly in these tests.
// The staging relay will always be used, to avoid unnecessary load on prod relay.
// The localhost Postgres will always be used. This is valid in both docker-compose.storage and GitHub CI.

// TODO make these DRY with local configuration defaults
fn get_vars() -> Vars {
    Vars {
        project_id: env::var("PROJECT_ID").unwrap(),

        // No use-case to modify these currently.
        relay_url: "wss://staging.relay.walletconnect.com".to_owned(),
    }
}

struct Vars {
    project_id: String,
    relay_url: String,
}

async fn get_postgres() -> (PgPool, String) {
    let base_url = "postgres://postgres:password@localhost:5432";

    let postgres = PgPoolOptions::new()
        .connect(&format!("{base_url}/postgres"))
        .await
        .unwrap();
    let encoding = {
        let mut spec = data_encoding::Specification::new();
        spec.symbols.push_str("abcdefghijklmnop");
        spec.encoding().unwrap()
    };
    let db_name = encoding.encode(&rand::Rng::gen::<[u8; 10]>(&mut rand::thread_rng()));
    sqlx::query(&format!("CREATE DATABASE {db_name}"))
        .execute(&postgres)
        .await
        .unwrap();

    let postgres_url = format!("{base_url}/{db_name}");
    let postgres = PgPoolOptions::new().connect(&postgres_url).await.unwrap();

    let mut txn = postgres.begin().await.unwrap();
    sqlx::query("DROP SCHEMA IF EXISTS public CASCADE")
        .execute(&mut *txn)
        .await
        .unwrap();
    sqlx::query("CREATE SCHEMA public")
        .execute(&mut *txn)
        .await
        .unwrap();
    txn.commit().await.unwrap();

    sqlx::migrate!("./migrations").run(&postgres).await.unwrap();

    (postgres, postgres_url)
}

fn generate_app_domain() -> String {
    format!(
        "{}.example.com",
        hex::encode(rand::Rng::gen::<[u8; 10]>(&mut rand::thread_rng()))
    )
}

fn generate_subscribe_key() -> x25519_dalek::StaticSecret {
    x25519_dalek::StaticSecret::random_from_rng(OsRng)
}

fn generate_authentication_key() -> ed25519_dalek::SigningKey {
    ed25519_dalek::SigningKey::generate(&mut OsRng)
}

fn generate_account_id() -> AccountId {
    generate_account().1
}

#[tokio::test]
async fn test_one_project() {
    let (postgres, _) = get_postgres().await;

    let topic = Topic::generate();
    let project_id = ProjectId::generate();
    let subscribe_key = generate_subscribe_key();
    let authentication_key = generate_authentication_key();
    let app_domain = generate_app_domain();
    upsert_project(
        project_id.clone(),
        &app_domain,
        topic.clone(),
        &authentication_key,
        &subscribe_key,
        &postgres,
        None,
    )
    .await
    .unwrap();

    assert_eq!(
        get_subscriber_topics(&postgres, None).await.unwrap(),
        vec![]
    );
    assert_eq!(
        get_project_topics(&postgres, None).await.unwrap(),
        vec![topic.clone()]
    );
    let project = get_project_by_app_domain(&app_domain, &postgres, None)
        .await
        .unwrap();
    assert_eq!(project.project_id, project_id.clone());
    assert_eq!(project.app_domain, app_domain);
    assert_eq!(project.topic, topic);
    assert_eq!(
        project.authentication_public_key,
        encode_authentication_public_key(&authentication_key),
    );
    assert_eq!(
        project.authentication_private_key,
        encode_authentication_private_key(&authentication_key),
    );
    assert_eq!(
        project.subscribe_public_key,
        encode_subscribe_public_key(&subscribe_key)
    );
    assert_eq!(
        project.subscribe_private_key,
        encode_subscribe_private_key(&subscribe_key),
    );

    let project = get_project_by_project_id(project_id.clone(), &postgres, None)
        .await
        .unwrap();
    assert_eq!(project.project_id, project_id.clone());
    assert_eq!(project.app_domain, app_domain);
    assert_eq!(project.topic, topic);
    assert_eq!(
        project.authentication_public_key,
        encode_authentication_public_key(&authentication_key),
    );
    assert_eq!(
        project.authentication_private_key,
        encode_authentication_private_key(&authentication_key),
    );
    assert_eq!(
        project.subscribe_public_key,
        encode_subscribe_public_key(&subscribe_key)
    );
    assert_eq!(
        project.subscribe_private_key,
        encode_subscribe_private_key(&subscribe_key),
    );

    let project = get_project_by_topic(topic.clone(), &postgres, None)
        .await
        .unwrap();
    assert_eq!(project.project_id, project_id.clone());
    assert_eq!(project.app_domain, app_domain);
    assert_eq!(project.topic, topic);
    assert_eq!(
        project.authentication_public_key,
        encode_authentication_public_key(&authentication_key),
    );
    assert_eq!(
        project.authentication_private_key,
        encode_authentication_private_key(&authentication_key),
    );
    assert_eq!(
        project.subscribe_public_key,
        encode_subscribe_public_key(&subscribe_key)
    );
    assert_eq!(
        project.subscribe_private_key,
        encode_subscribe_private_key(&subscribe_key),
    );
}

#[tokio::test]
async fn test_one_subscriber() {
    let (postgres, _) = get_postgres().await;

    let topic = Topic::generate();
    let project_id = ProjectId::generate();
    let subscribe_key = generate_subscribe_key();
    let authentication_key = generate_authentication_key();
    let app_domain = generate_app_domain();
    upsert_project(
        project_id.clone(),
        &app_domain,
        topic,
        &authentication_key,
        &subscribe_key,
        &postgres,
        None,
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &postgres, None)
        .await
        .unwrap();

    let account_id = generate_account_id();
    let subscriber_sym_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let subscriber_topic: Topic = sha256::digest(&subscriber_sym_key).into();
    let subscriber_scope = HashSet::from([Uuid::new_v4(), Uuid::new_v4()]);
    upsert_subscriber(
        project.id,
        account_id.clone(),
        subscriber_scope.clone(),
        &subscriber_sym_key,
        subscriber_topic.clone(),
        &postgres,
        None,
    )
    .await
    .unwrap();
    // let subscriber_scope = subscriber_scope.map(|s| s.to_string()).collect::<HashSet<_>>();

    assert_eq!(
        get_subscriber_topics(&postgres, None).await.unwrap(),
        vec![subscriber_topic.clone()]
    );

    let subscriber = get_subscriber_by_topic(subscriber_topic.clone(), &postgres, None)
        .await
        .unwrap();
    assert_eq!(subscriber.project, project.id);
    assert_eq!(subscriber.account, account_id);
    assert_eq!(subscriber.sym_key, hex::encode(subscriber_sym_key));
    assert_eq!(subscriber.topic, subscriber_topic);
    assert_eq!(
        subscriber.scope.into_iter().collect::<HashSet<_>>(),
        subscriber_scope
    );
    assert!(subscriber.expiry > Utc::now() + Duration::days(29));

    let subscribers =
        get_subscribers_for_project_in(project.id, &[account_id.clone()], &postgres, None)
            .await
            .unwrap();
    assert_eq!(subscribers.len(), 1);
    let subscriber = &subscribers[0];
    assert_eq!(subscriber.account, account_id);
    assert_eq!(subscriber.scope, subscriber_scope);

    let accounts = get_subscriber_accounts_by_project_id(project_id.clone(), &postgres, None)
        .await
        .unwrap();
    assert_eq!(accounts, vec![account_id.clone()]);

    let subscribers = get_subscriptions_by_account(account_id.clone(), &postgres, None)
        .await
        .unwrap();
    assert_eq!(subscribers.len(), 1);
    let subscriber = &subscribers[0];
    assert_eq!(subscriber.app_domain, project.app_domain);
    assert_eq!(subscriber.account, account_id);
    assert_eq!(subscriber.sym_key, hex::encode(subscriber_sym_key));
    assert_eq!(subscriber.scope, subscriber_scope);
    assert!(subscriber.expiry > Utc::now() + Duration::days(29));
}

#[tokio::test]
async fn test_two_subscribers() {
    let (postgres, _) = get_postgres().await;

    let topic = Topic::generate();
    let project_id = ProjectId::generate();
    let subscribe_key = generate_subscribe_key();
    let authentication_key = generate_authentication_key();
    let app_domain = generate_app_domain();
    upsert_project(
        project_id.clone(),
        &app_domain,
        topic,
        &authentication_key,
        &subscribe_key,
        &postgres,
        None,
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &postgres, None)
        .await
        .unwrap();

    let account_id = generate_account_id();
    let subscriber_sym_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let subscriber_topic: Topic = sha256::digest(&subscriber_sym_key).into();
    let subscriber_scope = HashSet::from([Uuid::new_v4(), Uuid::new_v4()]);
    upsert_subscriber(
        project.id,
        account_id.clone(),
        subscriber_scope.clone(),
        &subscriber_sym_key,
        subscriber_topic.clone(),
        &postgres,
        None,
    )
    .await
    .unwrap();

    let account_id2: AccountId = "eip155:1:0xEEE".into();
    let subscriber_sym_key2 = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let subscriber_topic2: Topic = sha256::digest(&subscriber_sym_key2).into();
    let subscriber_scope2 = HashSet::from([Uuid::new_v4(), Uuid::new_v4()]);
    upsert_subscriber(
        project.id,
        account_id2.clone(),
        subscriber_scope2.clone(),
        &subscriber_sym_key2,
        subscriber_topic2.clone(),
        &postgres,
        None,
    )
    .await
    .unwrap();

    let project = get_project_by_project_id(project_id.clone(), &postgres, None)
        .await
        .unwrap();

    assert_eq!(
        get_subscriber_topics(&postgres, None)
            .await
            .unwrap()
            .into_iter()
            .collect::<HashSet<_>>(),
        HashSet::from([subscriber_topic.clone(), subscriber_topic2.clone()])
    );

    let subscriber = get_subscriber_by_topic(subscriber_topic.clone(), &postgres, None)
        .await
        .unwrap();
    assert_eq!(subscriber.project, project.id);
    assert_eq!(subscriber.account, account_id);
    assert_eq!(subscriber.sym_key, hex::encode(subscriber_sym_key));
    assert_eq!(subscriber.topic, subscriber_topic);
    assert_eq!(
        subscriber.scope.into_iter().collect::<HashSet<_>>(),
        subscriber_scope
    );
    assert!(subscriber.expiry > Utc::now() + Duration::days(29));

    let subscriber = get_subscriber_by_topic(subscriber_topic2.clone(), &postgres, None)
        .await
        .unwrap();
    assert_eq!(subscriber.project, project.id);
    assert_eq!(subscriber.account, account_id2);
    assert_eq!(subscriber.sym_key, hex::encode(subscriber_sym_key2));
    assert_eq!(subscriber.topic, subscriber_topic2);
    assert_eq!(
        subscriber.scope.into_iter().collect::<HashSet<_>>(),
        subscriber_scope2
    );
    assert!(subscriber.expiry > Utc::now() + Duration::days(29));

    let subscribers = get_subscribers_for_project_in(
        project.id,
        &[account_id.clone(), account_id2.clone()],
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(subscribers.len(), 2);
    for subscriber in subscribers {
        if subscriber.account == account_id {
            assert_eq!(subscriber.scope, subscriber_scope);
        } else {
            assert_eq!(subscriber.scope, subscriber_scope2);
        }
    }

    let accounts = get_subscriber_accounts_by_project_id(project_id.clone(), &postgres, None)
        .await
        .unwrap();
    assert_eq!(
        accounts.iter().cloned().collect::<HashSet<_>>(),
        HashSet::from([account_id.clone(), account_id2.clone()])
    );

    let subscribers = get_subscriptions_by_account(account_id.clone(), &postgres, None)
        .await
        .unwrap();
    assert_eq!(subscribers.len(), 1);
    let subscriber = &subscribers[0];
    assert_eq!(subscriber.app_domain, project.app_domain);
    assert_eq!(subscriber.account, account_id);
    assert_eq!(subscriber.sym_key, hex::encode(subscriber_sym_key));
    assert_eq!(subscriber.scope, subscriber_scope);
    assert!(subscriber.expiry > Utc::now() + Duration::days(29));

    let subscribers = get_subscriptions_by_account(account_id2.clone(), &postgres, None)
        .await
        .unwrap();
    assert_eq!(subscribers.len(), 1);
    let subscriber = &subscribers[0];
    assert_eq!(subscriber.app_domain, project.app_domain);
    assert_eq!(subscriber.account, account_id2);
    assert_eq!(subscriber.sym_key, hex::encode(subscriber_sym_key2));
    assert_eq!(subscriber.scope, subscriber_scope2);
    assert!(subscriber.expiry > Utc::now() + Duration::days(29));
}

#[tokio::test]
async fn test_one_subscriber_two_projects() {
    let (postgres, _) = get_postgres().await;

    let topic = Topic::generate();
    let project_id = ProjectId::generate();
    let subscribe_key = generate_subscribe_key();
    let authentication_key = generate_authentication_key();
    let app_domain = generate_app_domain();
    upsert_project(
        project_id.clone(),
        &app_domain,
        topic,
        &authentication_key,
        &subscribe_key,
        &postgres,
        None,
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &postgres, None)
        .await
        .unwrap();

    let topic2 = Topic::generate();
    let project_id2 = ProjectId::generate();
    let subscribe_key2 = generate_subscribe_key();
    let authentication_key2 = generate_authentication_key();
    let app_domain2 = generate_app_domain();
    upsert_project(
        project_id2.clone(),
        &app_domain2,
        topic2,
        &authentication_key2,
        &subscribe_key2,
        &postgres,
        None,
    )
    .await
    .unwrap();
    let project2 = get_project_by_project_id(project_id2.clone(), &postgres, None)
        .await
        .unwrap();

    let account_id = generate_account_id();
    let subscriber_sym_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let subscriber_topic: Topic = sha256::digest(&subscriber_sym_key).into();
    let subscriber_scope = HashSet::from([Uuid::new_v4(), Uuid::new_v4()]);
    upsert_subscriber(
        project.id,
        account_id.clone(),
        subscriber_scope.clone(),
        &subscriber_sym_key,
        subscriber_topic.clone(),
        &postgres,
        None,
    )
    .await
    .unwrap();
    let subscriber_sym_key2 = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let subscriber_topic2: Topic = sha256::digest(&subscriber_sym_key2).into();
    let subscriber_scope2 = HashSet::from([Uuid::new_v4(), Uuid::new_v4()]);
    upsert_subscriber(
        project2.id,
        account_id.clone(),
        subscriber_scope2.clone(),
        &subscriber_sym_key2,
        subscriber_topic2.clone(),
        &postgres,
        None,
    )
    .await
    .unwrap();

    let project = get_project_by_project_id(project_id.clone(), &postgres, None)
        .await
        .unwrap();
    let project2 = get_project_by_project_id(project_id2.clone(), &postgres, None)
        .await
        .unwrap();

    assert_eq!(
        get_subscriber_topics(&postgres, None)
            .await
            .unwrap()
            .into_iter()
            .collect::<HashSet<_>>(),
        HashSet::from([subscriber_topic.clone(), subscriber_topic2.clone()])
    );

    let subscriber = get_subscriber_by_topic(subscriber_topic.clone(), &postgres, None)
        .await
        .unwrap();
    assert_eq!(subscriber.project, project.id);
    assert_eq!(subscriber.account, account_id);
    assert_eq!(subscriber.sym_key, hex::encode(subscriber_sym_key));
    assert_eq!(subscriber.topic, subscriber_topic);
    assert_eq!(
        subscriber.scope.into_iter().collect::<HashSet<_>>(),
        subscriber_scope
    );
    assert!(subscriber.expiry > Utc::now() + Duration::days(29));

    let subscriber = get_subscriber_by_topic(subscriber_topic2.clone(), &postgres, None)
        .await
        .unwrap();
    assert_eq!(subscriber.project, project2.id);
    assert_eq!(subscriber.account, account_id);
    assert_eq!(subscriber.sym_key, hex::encode(subscriber_sym_key2));
    assert_eq!(subscriber.topic, subscriber_topic2);
    assert_eq!(
        subscriber.scope.into_iter().collect::<HashSet<_>>(),
        subscriber_scope2
    );
    assert!(subscriber.expiry > Utc::now() + Duration::days(29));

    let subscribers =
        get_subscribers_for_project_in(project.id, &[account_id.clone()], &postgres, None)
            .await
            .unwrap();
    assert_eq!(subscribers.len(), 1);
    let subscriber = &subscribers[0];
    assert_eq!(subscriber.account, account_id);
    assert_eq!(subscriber.scope, subscriber_scope);

    let subscribers =
        get_subscribers_for_project_in(project2.id, &[account_id.clone()], &postgres, None)
            .await
            .unwrap();
    assert_eq!(subscribers.len(), 1);
    let subscriber = &subscribers[0];
    assert_eq!(subscriber.account, account_id);
    assert_eq!(subscriber.scope, subscriber_scope2);

    let accounts = get_subscriber_accounts_by_project_id(project_id.clone(), &postgres, None)
        .await
        .unwrap();
    assert_eq!(accounts, vec![account_id.clone()]);
    let accounts = get_subscriber_accounts_by_project_id(project_id2.clone(), &postgres, None)
        .await
        .unwrap();
    assert_eq!(accounts, vec![account_id.clone()]);

    let subscribers = get_subscriptions_by_account(account_id.clone(), &postgres, None)
        .await
        .unwrap();
    assert_eq!(subscribers.len(), 2);
    for subscriber in subscribers {
        if subscriber.app_domain == app_domain {
            assert_eq!(subscriber.app_domain, app_domain);
            assert_eq!(subscriber.account, account_id);
            assert_eq!(subscriber.sym_key, hex::encode(subscriber_sym_key));
            assert_eq!(subscriber.scope, subscriber_scope);
            assert!(subscriber.expiry > Utc::now() + Duration::days(29));
        } else {
            assert_eq!(subscriber.app_domain, app_domain2);
            assert_eq!(subscriber.account, account_id);
            assert_eq!(subscriber.sym_key, hex::encode(subscriber_sym_key2));
            assert_eq!(subscriber.scope, subscriber_scope2);
            assert!(subscriber.expiry > Utc::now() + Duration::days(29));
        }
    }
}

#[tokio::test]
async fn test_account_case_insensitive() {
    let (postgres, _) = get_postgres().await;

    let topic = Topic::generate();
    let project_id = ProjectId::generate();
    let subscribe_key = generate_subscribe_key();
    let authentication_key = generate_authentication_key();
    let app_domain = &generate_app_domain();
    upsert_project(
        project_id.clone(),
        app_domain,
        topic,
        &authentication_key,
        &subscribe_key,
        &postgres,
        None,
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &postgres, None)
        .await
        .unwrap();

    let addr_prefix = generate_account_id();
    let account: AccountId = format!("{addr_prefix}fff").into();
    let scope = HashSet::from([Uuid::new_v4(), Uuid::new_v4()]);
    let notify_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let notify_topic = sha256::digest(&notify_key).into();
    upsert_subscriber(
        project.id,
        account.clone(),
        scope,
        &notify_key,
        notify_topic,
        &postgres,
        None,
    )
    .await
    .unwrap();

    let subscribers =
        get_subscriptions_by_account(format!("{addr_prefix}FFF").into(), &postgres, None)
            .await
            .unwrap();
    assert_eq!(subscribers.len(), 1);
}

#[tokio::test]
async fn test_get_subscriber_accounts_by_project_id() {
    let (postgres, _) = get_postgres().await;

    let project_id = ProjectId::generate();
    let app_domain = &generate_app_domain();
    let topic = Topic::generate();
    let subscribe_key = generate_subscribe_key();
    let authentication_key = generate_authentication_key();
    upsert_project(
        project_id.clone(),
        app_domain,
        topic,
        &authentication_key,
        &subscribe_key,
        &postgres,
        None,
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &postgres, None)
        .await
        .unwrap();

    let account = generate_account_id();
    let scope = HashSet::from([Uuid::new_v4(), Uuid::new_v4()]);
    let notify_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let notify_topic = sha256::digest(&notify_key).into();
    upsert_subscriber(
        project.id,
        account.clone(),
        scope,
        &notify_key,
        notify_topic,
        &postgres,
        None,
    )
    .await
    .unwrap();

    let accounts = get_subscriber_accounts_by_project_id(project_id, &postgres, None)
        .await
        .unwrap();
    assert_eq!(accounts, vec![account]);
}

#[tokio::test]
async fn test_get_subscriber_accounts_and_scopes_by_project_id() {
    let (postgres, _) = get_postgres().await;

    let project_id = ProjectId::generate();
    let app_domain = &generate_app_domain();
    let topic = Topic::generate();
    let subscribe_key = generate_subscribe_key();
    let authentication_key = generate_authentication_key();
    upsert_project(
        project_id.clone(),
        app_domain,
        topic,
        &authentication_key,
        &subscribe_key,
        &postgres,
        None,
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &postgres, None)
        .await
        .unwrap();

    let account = generate_account_id();
    let scope = HashSet::from([Uuid::new_v4(), Uuid::new_v4()]);
    let notify_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let notify_topic = sha256::digest(&notify_key).into();
    upsert_subscriber(
        project.id,
        account.clone(),
        scope.clone(),
        &notify_key,
        notify_topic,
        &postgres,
        None,
    )
    .await
    .unwrap();

    let subscribers = get_subscriber_accounts_and_scopes_by_project_id(project_id, &postgres, None)
        .await
        .unwrap();
    assert_eq!(
        subscribers,
        vec![SubscriberAccountAndScopes { account, scope }]
    );
}

async fn is_socket_addr_available<A: ToSocketAddrs>(socket_addr: A) -> bool {
    TcpListener::bind(socket_addr).await.is_ok()
}

async fn find_free_port(bind_ip: IpAddr) -> u16 {
    use std::sync::atomic::{AtomicU16, Ordering};
    static NEXT_PORT: AtomicU16 = AtomicU16::new(9001);
    loop {
        let port = NEXT_PORT.fetch_add(1, Ordering::SeqCst);
        if is_socket_addr_available((bind_ip, port)).await {
            return port;
        }
    }
}

async fn wait_for_socket_addr_to_be(socket_addr: SocketAddr, open: bool) -> Result<(), Elapsed> {
    use {std::time::Duration, tokio::time};
    time::timeout(Duration::from_secs(3), async {
        while is_socket_addr_available(socket_addr).await != open {
            time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
}

struct NotifyServerContext {
    shutdown: broadcast::Sender<()>,
    socket_addr: SocketAddr,
    url: Url,
    postgres: PgPool,
    redis: Arc<Redis>,
    #[allow(dead_code)] // must hold onto MockServer reference or it will shut down
    registry_mock_server: MockServer,
}

#[async_trait]
impl AsyncTestContext for NotifyServerContext {
    async fn setup() -> Self {
        let registry_mock_server = {
            use wiremock::{
                http::Method,
                matchers::{method, path},
                Mock, MockServer, ResponseTemplate,
            };
            let mock_server = MockServer::start().await;
            Mock::given(method(Method::Get))
                .and(path("/internal/project/validate-notify-keys"))
                .respond_with(
                    ResponseTemplate::new(StatusCode::OK)
                        .set_body_json(RegistryAuthResponse { is_valid: true }),
                )
                .mount(&mock_server)
                .await;
            mock_server
        };

        let vars = get_vars();
        let bind_ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let bind_port = find_free_port(bind_ip).await;
        let telemetry_prometheus_port = find_free_port(bind_ip).await;
        let socket_addr = SocketAddr::from((bind_ip, bind_port));
        let notify_url = format!("http://{socket_addr}").parse::<Url>().unwrap();
        let (_, postgres_url) = get_postgres().await;
        // TODO reuse the local configuration defaults here
        let config = Configuration {
            postgres_url,
            postgres_max_connections: 10,
            log_level: "DEBUG".to_string(),
            public_ip: bind_ip,
            bind_ip,
            port: bind_port,
            registry_url: registry_mock_server.uri().parse().unwrap(),
            keypair_seed: hex::encode(rand::Rng::gen::<[u8; 10]>(&mut rand::thread_rng())),
            project_id: vars.project_id.into(),
            relay_url: vars.relay_url.parse().unwrap(),
            notify_url: notify_url.clone(),
            registry_auth_token: "".to_owned(),
            auth_redis_addr_read: Some("redis://localhost:6379/0".to_owned()),
            auth_redis_addr_write: Some("redis://localhost:6379/0".to_owned()),
            redis_pool_size: 1,
            telemetry_prometheus_port: Some(telemetry_prometheus_port),
            s3_endpoint: None,
            geoip_db_bucket: None,
            geoip_db_key: None,
            blocked_countries: vec![],
            analytics_export_bucket: None,
        };
        tracing_subscriber::fmt()
            .with_env_filter(&config.log_level)
            .with_span_events(FmtSpan::CLOSE)
            .with_ansi(std::env::var("ANSI_LOGS").is_ok())
            .try_init()
            .ok();

        let (signal, shutdown) = broadcast::channel(1);
        tokio::task::spawn({
            let config = config.clone();
            async move {
                notify_server::bootstrap(shutdown, config).await.unwrap();
            }
        });

        wait_for_socket_addr_to_be(socket_addr, false)
            .await
            .unwrap();

        let postgres = PgPoolOptions::new()
            .connect(&config.postgres_url)
            .await
            .unwrap();

        let redis = Arc::new(
            Redis::new(
                &config.auth_redis_addr().unwrap(),
                config.redis_pool_size as usize,
            )
            .unwrap(),
        );

        Self {
            shutdown: signal,
            socket_addr,
            url: notify_url,
            postgres,
            redis,
            registry_mock_server,
        }
    }

    async fn teardown(mut self) {
        self.shutdown.send(()).unwrap();
        wait_for_socket_addr_to_be(self.socket_addr, true)
            .await
            .unwrap();
    }
}

async fn assert_successful_response(response: Response) -> Response {
    let status = response.status();
    if !status.is_success() {
        panic!(
            "non-successful response {status}: {:?}",
            response.text().await
        );
    }
    response
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn test_get_subscribers_v0(notify_server: &NotifyServerContext) {
    let project_id = ProjectId::generate();
    let app_domain = &generate_app_domain();
    let topic = Topic::generate();
    let subscribe_key = generate_subscribe_key();
    let authentication_key = generate_authentication_key();
    upsert_project(
        project_id.clone(),
        app_domain,
        topic,
        &authentication_key,
        &subscribe_key,
        &notify_server.postgres,
        None,
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &notify_server.postgres, None)
        .await
        .unwrap();

    let account = generate_account_id();
    let scope = HashSet::from([Uuid::new_v4(), Uuid::new_v4()]);
    let notify_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let notify_topic = sha256::digest(&notify_key).into();
    upsert_subscriber(
        project.id,
        account.clone(),
        scope,
        &notify_key,
        notify_topic,
        &notify_server.postgres,
        None,
    )
    .await
    .unwrap();

    let accounts =
        get_subscriber_accounts_by_project_id(project_id.clone(), &notify_server.postgres, None)
            .await
            .unwrap();
    assert_eq!(accounts, vec![account.clone()]);

    let accounts = assert_successful_response(
        reqwest::Client::new()
            .get(
                notify_server
                    .url
                    .join(&format!("/{project_id}/subscribers"))
                    .unwrap(),
            )
            .header("Authorization", format!("Bearer {}", Uuid::new_v4()))
            .send()
            .await
            .unwrap(),
    )
    .await
    .json::<Vec<AccountId>>()
    .await
    .unwrap();
    assert_eq!(accounts, vec![account]);
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn test_get_subscribers_v1(notify_server: &NotifyServerContext) {
    let project_id = ProjectId::generate();
    let app_domain = &generate_app_domain();
    let topic = Topic::generate();
    let subscribe_key = generate_subscribe_key();
    let authentication_key = generate_authentication_key();
    upsert_project(
        project_id.clone(),
        app_domain,
        topic,
        &authentication_key,
        &subscribe_key,
        &notify_server.postgres,
        None,
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &notify_server.postgres, None)
        .await
        .unwrap();

    let account = generate_account_id();
    let scope = HashSet::from([Uuid::new_v4(), Uuid::new_v4()]);
    let notify_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let notify_topic = sha256::digest(&notify_key).into();
    upsert_subscriber(
        project.id,
        account.clone(),
        scope.clone(),
        &notify_key,
        notify_topic,
        &notify_server.postgres,
        None,
    )
    .await
    .unwrap();

    let subscribers = get_subscriber_accounts_and_scopes_by_project_id(
        project_id.clone(),
        &notify_server.postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        subscribers,
        vec![SubscriberAccountAndScopes {
            account: account.clone(),
            scope: scope.clone()
        }]
    );

    let subscribers = assert_successful_response(
        reqwest::Client::new()
            .get(
                notify_server
                    .url
                    .join(&format!("/v1/{project_id}/subscribers"))
                    .unwrap(),
            )
            .header("Authorization", format!("Bearer {}", Uuid::new_v4()))
            .send()
            .await
            .unwrap(),
    )
    .await
    .json::<Vec<SubscriberAccountAndScopes>>()
    .await
    .unwrap();
    assert_eq!(
        subscribers,
        vec![SubscriberAccountAndScopes { account, scope }]
    );
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn test_notify_v0(notify_server: &NotifyServerContext) {
    let project_id = ProjectId::generate();
    let app_domain = generate_app_domain();
    let topic = Topic::generate();
    let subscribe_key = generate_subscribe_key();
    let authentication_key = generate_authentication_key();
    upsert_project(
        project_id.clone(),
        &app_domain,
        topic,
        &authentication_key,
        &subscribe_key,
        &notify_server.postgres,
        None,
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &notify_server.postgres, None)
        .await
        .unwrap();

    let account = generate_account_id();
    let notification_type = Uuid::new_v4();
    let scope = HashSet::from([notification_type]);
    let notify_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let notify_topic: Topic = sha256::digest(&notify_key).into();
    upsert_subscriber(
        project.id,
        account.clone(),
        scope.clone(),
        &notify_key,
        notify_topic.clone(),
        &notify_server.postgres,
        None,
    )
    .await
    .unwrap();

    let vars = get_vars();
    let (relay_ws_client, mut rx) = create_client(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    relay_ws_client.subscribe(notify_topic).await.unwrap();

    let notification = Notification {
        r#type: notification_type,
        title: "title".to_owned(),
        body: "body".to_owned(),
        icon: None,
        url: None,
    };

    let notify_body = NotifyBody {
        notification_id: None,
        notification: notification.clone(),
        accounts: vec![account.clone()],
    };

    let notify_url = notify_server
        .url
        .join(&format!("{project_id}/notify"))
        .unwrap();
    assert_successful_response(
        reqwest::Client::new()
            .post(notify_url)
            .bearer_auth(Uuid::new_v4())
            .json(&notify_body)
            .send()
            .await
            .unwrap(),
    )
    .await;

    let resp = rx.recv().await.unwrap();
    let RelayClientEvent::Message(msg) = resp else {
        panic!("Expected message, got {:?}", resp);
    };
    assert_eq!(msg.tag, NOTIFY_MESSAGE_TAG);

    let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(&notify_key));

    let Envelope::<EnvelopeType0> { iv, sealbox, .. } = Envelope::<EnvelopeType0>::from_bytes(
        base64::engine::general_purpose::STANDARD
            .decode(msg.message.as_bytes())
            .unwrap(),
    )
    .unwrap();

    // TODO: add proper type for that val
    let decrypted_notification: NotifyRequest<NotifyPayload> = serde_json::from_slice(
        &cipher
            .decrypt(&iv.into(), chacha20poly1305::aead::Payload::from(&*sealbox))
            .unwrap(),
    )
    .unwrap();

    // let received_notification = decrypted_notification.params;
    let claims = verify_jwt(
        &decrypted_notification.params.message_auth,
        &authentication_key.verifying_key(),
    )
    .unwrap();

    // https://github.com/WalletConnect/walletconnect-docs/blob/main/docs/specs/clients/notify/notify-authentication.md#notify-message
    // TODO: verify issuer
    assert_eq!(claims.msg.r#type, notification_type);
    assert_eq!(claims.msg.title, "title");
    assert_eq!(claims.msg.body, "body");
    assert_eq!(claims.msg.icon, "");
    assert_eq!(claims.msg.url, "");
    assert!(claims.iat < chrono::Utc::now().timestamp() + JWT_LEEWAY); // TODO remove leeway
    assert!(claims.exp > chrono::Utc::now().timestamp() - JWT_LEEWAY); // TODO remove leeway
    assert_eq!(claims.app.as_ref(), app_domain);
    assert_eq!(claims.sub, account.to_did_pkh());
    assert_eq!(claims.act, "notify_message");
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn test_notify_v1(notify_server: &NotifyServerContext) {
    let project_id = ProjectId::generate();
    let app_domain = generate_app_domain();
    let topic = Topic::generate();
    let subscribe_key = generate_subscribe_key();
    let authentication_key = generate_authentication_key();
    upsert_project(
        project_id.clone(),
        &app_domain,
        topic,
        &authentication_key,
        &subscribe_key,
        &notify_server.postgres,
        None,
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &notify_server.postgres, None)
        .await
        .unwrap();

    let account = generate_account_id();
    let notification_type = Uuid::new_v4();
    let scope = HashSet::from([notification_type]);
    let notify_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let notify_topic: Topic = sha256::digest(&notify_key).into();
    upsert_subscriber(
        project.id,
        account.clone(),
        scope.clone(),
        &notify_key,
        notify_topic.clone(),
        &notify_server.postgres,
        None,
    )
    .await
    .unwrap();

    let vars = get_vars();
    let (relay_ws_client, mut rx) = create_client(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    relay_ws_client.subscribe(notify_topic).await.unwrap();

    let notification = Notification {
        r#type: notification_type,
        title: "title".to_owned(),
        body: "body".to_owned(),
        icon: Some("icon".to_owned()),
        url: Some("url".to_owned()),
    };

    let notification_body = NotifyBodyNotification {
        notification_id: None,
        notification: notification.clone(),
        accounts: vec![account.clone()],
    };
    let notify_body = vec![notification_body];

    let notify_url = notify_server
        .url
        .join(&format!("/v1/{project_id}/notify"))
        .unwrap();
    let response = assert_successful_response(
        reqwest::Client::new()
            .post(notify_url)
            .bearer_auth(Uuid::new_v4())
            .json(&notify_body)
            .send()
            .await
            .unwrap(),
    )
    .await
    .json::<notify_v1::Response>()
    .await
    .unwrap();
    assert!(response.not_found.is_empty());
    assert!(response.failed.is_empty());
    assert_eq!(response.sent, HashSet::from([account.clone()]));

    let resp = rx.recv().await.unwrap();
    let RelayClientEvent::Message(msg) = resp else {
        panic!("Expected message, got {:?}", resp);
    };
    assert_eq!(msg.tag, NOTIFY_MESSAGE_TAG);

    let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(&notify_key));

    let Envelope::<EnvelopeType0> { iv, sealbox, .. } = Envelope::<EnvelopeType0>::from_bytes(
        base64::engine::general_purpose::STANDARD
            .decode(msg.message.as_bytes())
            .unwrap(),
    )
    .unwrap();

    // TODO: add proper type for that val
    let decrypted_notification: NotifyRequest<NotifyPayload> = serde_json::from_slice(
        &cipher
            .decrypt(&iv.into(), chacha20poly1305::aead::Payload::from(&*sealbox))
            .unwrap(),
    )
    .unwrap();

    // let received_notification = decrypted_notification.params;
    let claims = verify_jwt(
        &decrypted_notification.params.message_auth,
        &authentication_key.verifying_key(),
    )
    .unwrap();

    // https://github.com/WalletConnect/walletconnect-docs/blob/main/docs/specs/clients/notify/notify-authentication.md#notify-message
    // TODO: verify issuer
    assert_eq!(claims.msg.r#type, notification.r#type);
    assert_eq!(claims.msg.title, notification.title);
    assert_eq!(claims.msg.body, notification.body);
    assert_eq!(claims.msg.icon, "icon");
    assert_eq!(claims.msg.url, "url");
    assert!(claims.iat < chrono::Utc::now().timestamp() + JWT_LEEWAY); // TODO remove leeway
    assert!(claims.exp > chrono::Utc::now().timestamp() - JWT_LEEWAY); // TODO remove leeway
    assert_eq!(claims.app.as_ref(), app_domain);
    assert_eq!(claims.sub, account.to_did_pkh());
    assert_eq!(claims.act, "notify_message");
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn test_notify_v1_response_not_found(notify_server: &NotifyServerContext) {
    let project_id = ProjectId::generate();
    let app_domain = generate_app_domain();
    let topic = Topic::generate();
    let subscribe_key = generate_subscribe_key();
    let authentication_key = generate_authentication_key();
    upsert_project(
        project_id.clone(),
        &app_domain,
        topic,
        &authentication_key,
        &subscribe_key,
        &notify_server.postgres,
        None,
    )
    .await
    .unwrap();

    let account = generate_account_id();
    let notification_type = Uuid::new_v4();

    let notification = Notification {
        r#type: notification_type,
        title: "title".to_owned(),
        body: "body".to_owned(),
        icon: Some("icon".to_owned()),
        url: Some("url".to_owned()),
    };

    let notification_body = NotifyBodyNotification {
        notification_id: None,
        notification: notification.clone(),
        accounts: vec![account.clone()],
    };
    let notify_body = vec![notification_body];

    let notify_url = notify_server
        .url
        .join(&format!("/v1/{project_id}/notify"))
        .unwrap();
    let response = assert_successful_response(
        reqwest::Client::new()
            .post(notify_url)
            .bearer_auth(Uuid::new_v4())
            .json(&notify_body)
            .send()
            .await
            .unwrap(),
    )
    .await
    .json::<notify_v1::Response>()
    .await
    .unwrap();
    assert_eq!(response.not_found, HashSet::from([account.clone()]));
    assert!(response.failed.is_empty());
    assert!(response.sent.is_empty());
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn test_notify_v1_response_not_subscribed_to_scope(notify_server: &NotifyServerContext) {
    let project_id = ProjectId::generate();
    let app_domain = generate_app_domain();
    let topic = Topic::generate();
    let subscribe_key = generate_subscribe_key();
    let authentication_key = generate_authentication_key();
    upsert_project(
        project_id.clone(),
        &app_domain,
        topic,
        &authentication_key,
        &subscribe_key,
        &notify_server.postgres,
        None,
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &notify_server.postgres, None)
        .await
        .unwrap();

    let account = generate_account_id();
    let notification_type = Uuid::new_v4();
    let scope = HashSet::from([notification_type]);
    let notify_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let notify_topic: Topic = sha256::digest(&notify_key).into();
    upsert_subscriber(
        project.id,
        account.clone(),
        scope.clone(),
        &notify_key,
        notify_topic.clone(),
        &notify_server.postgres,
        None,
    )
    .await
    .unwrap();

    let notification = Notification {
        r#type: Uuid::new_v4(),
        title: "title".to_owned(),
        body: "body".to_owned(),
        icon: Some("icon".to_owned()),
        url: Some("url".to_owned()),
    };

    let notification_body = NotifyBodyNotification {
        notification_id: None,
        notification: notification.clone(),
        accounts: vec![account.clone()],
    };
    let notify_body = vec![notification_body];

    let notify_url = notify_server
        .url
        .join(&format!("/v1/{project_id}/notify"))
        .unwrap();
    let response = assert_successful_response(
        reqwest::Client::new()
            .post(notify_url)
            .bearer_auth(Uuid::new_v4())
            .json(&notify_body)
            .send()
            .await
            .unwrap(),
    )
    .await
    .json::<notify_v1::Response>()
    .await
    .unwrap();
    assert!(response.not_found.is_empty());
    assert_eq!(
        response.failed,
        HashSet::from([notify_v1::SendFailure {
            account: account.clone(),
            reason: "Client is not subscribed to this notification type".into(),
        }])
    );
    assert!(response.sent.is_empty());
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn test_notify_idempotent(notify_server: &NotifyServerContext) {
    let project_id = ProjectId::generate();
    let app_domain = generate_app_domain();
    let topic = Topic::generate();
    let subscribe_key = generate_subscribe_key();
    let authentication_key = generate_authentication_key();
    upsert_project(
        project_id.clone(),
        &app_domain,
        topic,
        &authentication_key,
        &subscribe_key,
        &notify_server.postgres,
        None,
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &notify_server.postgres, None)
        .await
        .unwrap();

    let account = generate_account_id();
    let notification_type = Uuid::new_v4();
    let scope = HashSet::from([notification_type]);
    let notify_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let notify_topic: Topic = sha256::digest(&notify_key).into();
    upsert_subscriber(
        project.id,
        account.clone(),
        scope.clone(),
        &notify_key,
        notify_topic.clone(),
        &notify_server.postgres,
        None,
    )
    .await
    .unwrap();

    let notify_body = vec![NotifyBodyNotification {
        notification_id: Some(Uuid::new_v4().to_string()),
        notification: Notification {
            r#type: notification_type,
            title: "title".to_owned(),
            body: "body".to_owned(),
            icon: Some("icon".to_owned()),
            url: Some("url".to_owned()),
        },
        accounts: vec![account.clone()],
    }];

    let notify_url = notify_server
        .url
        .join(&format!("/v1/{project_id}/notify"))
        .unwrap();
    assert_successful_response(
        reqwest::Client::new()
            .post(notify_url.clone())
            .bearer_auth(Uuid::new_v4())
            .json(&notify_body)
            .send()
            .await
            .unwrap(),
    )
    .await;

    assert_successful_response(
        reqwest::Client::new()
            .post(notify_url)
            .bearer_auth(Uuid::new_v4())
            .json(&notify_body)
            .send()
            .await
            .unwrap(),
    )
    .await;
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn test_token_bucket(notify_server: &NotifyServerContext) {
    let key = Uuid::new_v4();
    let max_tokens = 2;
    let refill_interval = chrono::Duration::milliseconds(500);
    let refill_rate = 1;
    let rate_limit = || async {
        rate_limit::token_bucket_many(
            &notify_server.redis,
            vec![key.to_string()],
            max_tokens,
            refill_interval,
            refill_rate,
        )
        .await
        .unwrap()
        .get(&key.to_string())
        .unwrap()
        .to_owned()
    };

    let burst = || async {
        for tokens_remaining in (0..max_tokens).rev() {
            let result = rate_limit().await;
            assert_eq!(result.0, tokens_remaining as i64);
        }

        // Do it again, but fail, wait half a second and then it works again 1 time
        for _ in 0..2 {
            let result = rate_limit().await;
            assert!(result.0.is_negative());
            println!("result.1: {}", result.1);
            let refill_in = Utc
                .from_local_datetime(
                    &chrono::NaiveDateTime::from_timestamp_millis(result.1 as i64).unwrap(),
                )
                .unwrap()
                .signed_duration_since(Utc::now());
            println!("refill_in: {refill_in}");
            assert!(refill_in > chrono::Duration::zero());
            assert!(refill_in < refill_interval);

            tokio::time::sleep(refill_interval.to_std().unwrap()).await;

            let result = rate_limit().await;
            assert_eq!(result.0, 0);
        }
    };

    burst().await;

    // Let burst ability recover
    tokio::time::sleep(
        (refill_interval * (max_tokens / refill_rate) as i32)
            .to_std()
            .unwrap(),
    )
    .await;

    burst().await;
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn test_token_bucket_separate_keys(notify_server: &NotifyServerContext) {
    let rate_limit = |key: String| async move {
        rate_limit::token_bucket_many(
            &notify_server.redis,
            vec![key.clone()],
            2,
            chrono::Duration::seconds(5),
            1,
        )
        .await
        .unwrap()
        .get(&key)
        .unwrap()
        .to_owned()
    };

    let key1 = Uuid::new_v4();
    let key2 = Uuid::new_v4();

    let result = rate_limit(key1.to_string()).await;
    assert_eq!(result.0, 1);
    let result = rate_limit(key2.to_string()).await;
    assert_eq!(result.0, 1);
    let result = rate_limit(key2.to_string()).await;
    assert_eq!(result.0, 0);
    let result = rate_limit(key1.to_string()).await;
    assert_eq!(result.0, 0);
    let result = rate_limit(key2.to_string()).await;
    assert_eq!(result.0, -1);
    let result = rate_limit(key1.to_string()).await;
    assert_eq!(result.0, -1);
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn test_notify_rate_limit(notify_server: &NotifyServerContext) {
    let project_id = ProjectId::generate();
    let app_domain = generate_app_domain();
    let topic = Topic::generate();
    let subscribe_key = generate_subscribe_key();
    let authentication_key = generate_authentication_key();
    upsert_project(
        project_id.clone(),
        &app_domain,
        topic,
        &authentication_key,
        &subscribe_key,
        &notify_server.postgres,
        None,
    )
    .await
    .unwrap();

    let notification_type = Uuid::new_v4();
    let notify_body = vec![NotifyBodyNotification {
        notification_id: None,
        notification: Notification {
            r#type: notification_type,
            title: "title".to_owned(),
            body: "body".to_owned(),
            icon: None,
            url: None,
        },
        accounts: vec![],
    }];

    let notify_url = notify_server
        .url
        .join(&format!("/v1/{project_id}/notify"))
        .unwrap();
    let notify = || async {
        reqwest::Client::new()
            .post(notify_url.clone())
            .bearer_auth(Uuid::new_v4())
            .json(&notify_body)
            .send()
            .await
            .unwrap()
    };

    // Use up the rate limit
    while let Ok(()) = notify_rate_limit(&notify_server.redis, &project_id).await {}

    // No longer successful
    let response = notify().await;
    let status = response.status();
    if status != StatusCode::TOO_MANY_REQUESTS {
        panic!(
            "expected too many requests response, got {status}: {:?}",
            response.text().await
        );
    }
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn test_notify_subscriber_rate_limit(notify_server: &NotifyServerContext) {
    let project_id = ProjectId::generate();
    let app_domain = generate_app_domain();
    let topic = Topic::generate();
    let subscribe_key = generate_subscribe_key();
    let authentication_key = generate_authentication_key();
    upsert_project(
        project_id.clone(),
        &app_domain,
        topic,
        &authentication_key,
        &subscribe_key,
        &notify_server.postgres,
        None,
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &notify_server.postgres, None)
        .await
        .unwrap();

    let account = generate_account_id();
    let notification_type = Uuid::new_v4();
    let scope = HashSet::from([notification_type]);
    let notify_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let notify_topic: Topic = sha256::digest(&notify_key).into();
    let subscriber_id = upsert_subscriber(
        project.id,
        account.clone(),
        scope.clone(),
        &notify_key,
        notify_topic.clone(),
        &notify_server.postgres,
        None,
    )
    .await
    .unwrap();

    let notify_body = vec![NotifyBodyNotification {
        notification_id: None,
        notification: Notification {
            r#type: notification_type,
            title: "title".to_owned(),
            body: "body".to_owned(),
            icon: None,
            url: None,
        },
        accounts: vec![account.clone()],
    }];

    let notify_url = notify_server
        .url
        .join(&format!("/v1/{project_id}/notify"))
        .unwrap();
    let notify = || async {
        reqwest::Client::new()
            .post(notify_url.clone())
            .bearer_auth(Uuid::new_v4())
            .json(&notify_body)
            .send()
            .await
            .unwrap()
    };

    for _ in 0..49 {
        let result = subscriber_rate_limit(&notify_server.redis, &project_id, [subscriber_id])
            .await
            .unwrap();
        assert!(result
            .get(&subscriber_rate_limit_key(&project_id, &subscriber_id))
            .unwrap()
            .0
            .is_positive());
    }

    let response = assert_successful_response(notify().await)
        .await
        .json::<notify_v1::Response>()
        .await
        .unwrap();
    assert!(response.not_found.is_empty());
    assert!(response.failed.is_empty());
    assert_eq!(response.sent, HashSet::from([account.clone()]));

    let response = assert_successful_response(notify().await)
        .await
        .json::<notify_v1::Response>()
        .await
        .unwrap();
    assert!(response.not_found.is_empty());
    assert_eq!(
        response.failed,
        HashSet::from([notify_v1::SendFailure {
            account: account.clone(),
            reason: "Rate limit exceeded".into(),
        }])
    );
    assert!(response.sent.is_empty());
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn test_notify_subscriber_rate_limit_single(notify_server: &NotifyServerContext) {
    let project_id = ProjectId::generate();
    let app_domain = generate_app_domain();
    let topic = Topic::generate();
    let subscribe_key = generate_subscribe_key();
    let authentication_key = generate_authentication_key();
    upsert_project(
        project_id.clone(),
        &app_domain,
        topic,
        &authentication_key,
        &subscribe_key,
        &notify_server.postgres,
        None,
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &notify_server.postgres, None)
        .await
        .unwrap();

    let notification_type = Uuid::new_v4();

    let account1 = generate_account_id();
    let scope = HashSet::from([notification_type]);
    let notify_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let notify_topic: Topic = sha256::digest(&notify_key).into();
    let subscriber_id1 = upsert_subscriber(
        project.id,
        account1.clone(),
        scope.clone(),
        &notify_key,
        notify_topic.clone(),
        &notify_server.postgres,
        None,
    )
    .await
    .unwrap();

    let account2 = generate_account_id();
    let scope = HashSet::from([notification_type]);
    let notify_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let notify_topic: Topic = sha256::digest(&notify_key).into();
    let _subscriber_id2 = upsert_subscriber(
        project.id,
        account2.clone(),
        scope.clone(),
        &notify_key,
        notify_topic.clone(),
        &notify_server.postgres,
        None,
    )
    .await
    .unwrap();

    let notify_body = vec![NotifyBodyNotification {
        notification_id: None,
        notification: Notification {
            r#type: notification_type,
            title: "title".to_owned(),
            body: "body".to_owned(),
            icon: None,
            url: None,
        },
        accounts: vec![account1.clone(), account2.clone()],
    }];

    let notify_url = notify_server
        .url
        .join(&format!("/v1/{project_id}/notify"))
        .unwrap();
    let notify = || async {
        reqwest::Client::new()
            .post(notify_url.clone())
            .bearer_auth(Uuid::new_v4())
            .json(&notify_body)
            .send()
            .await
            .unwrap()
    };

    for _ in 0..49 {
        let result = subscriber_rate_limit(&notify_server.redis, &project_id, [subscriber_id1])
            .await
            .unwrap();
        assert!(result
            .get(&subscriber_rate_limit_key(&project_id, &subscriber_id1))
            .unwrap()
            .0
            .is_positive());
    }

    let response = assert_successful_response(notify().await)
        .await
        .json::<notify_v1::Response>()
        .await
        .unwrap();
    assert!(response.not_found.is_empty());
    assert!(response.failed.is_empty());
    assert_eq!(
        response.sent,
        HashSet::from([account1.clone(), account2.clone()])
    );

    let response = assert_successful_response(notify().await)
        .await
        .json::<notify_v1::Response>()
        .await
        .unwrap();
    assert!(response.not_found.is_empty());
    assert_eq!(
        response.failed,
        HashSet::from([notify_v1::SendFailure {
            account: account1.clone(),
            reason: "Rate limit exceeded".into(),
        }])
    );
    assert_eq!(response.sent, HashSet::from([account2.clone()]));
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn test_ignores_invalid_scopes(notify_server: &NotifyServerContext) {
    let project_id = ProjectId::generate();
    let app_domain = &generate_app_domain();
    let topic = Topic::generate();
    let subscribe_key = generate_subscribe_key();
    let authentication_key = generate_authentication_key();
    upsert_project(
        project_id.clone(),
        app_domain,
        topic,
        &authentication_key,
        &subscribe_key,
        &notify_server.postgres,
        None,
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &notify_server.postgres, None)
        .await
        .unwrap();

    let account = generate_account_id();
    let scope = HashSet::from([Uuid::new_v4(), Uuid::new_v4()]);
    let notify_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let notify_topic = sha256::digest(&notify_key).into();
    let subscriber = upsert_subscriber(
        project.id,
        account.clone(),
        scope.clone(),
        &notify_key,
        notify_topic,
        &notify_server.postgres,
        None,
    )
    .await
    .unwrap();

    // Test it ignores junk notification type
    let query = "INSERT INTO subscriber_scope ( subscriber, name ) VALUES ($1, $2);";
    let _ = sqlx::query::<Postgres>(query)
        .bind(subscriber)
        .bind("junk")
        .execute(&notify_server.postgres)
        .await
        .unwrap();

    let subscribers = get_subscriber_accounts_and_scopes_by_project_id(
        project_id.clone(),
        &notify_server.postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        subscribers,
        vec![SubscriberAccountAndScopes {
            account: account.clone(),
            scope: scope.clone()
        }]
    );

    // Test it doesn't ignore a UUID notification type
    let new_type = Uuid::new_v4();
    let query = "INSERT INTO subscriber_scope ( subscriber, name ) VALUES ($1, $2);";
    let _ = sqlx::query::<Postgres>(query)
        .bind(subscriber)
        .bind(new_type.to_string())
        .execute(&notify_server.postgres)
        .await
        .unwrap();

    let subscribers = get_subscriber_accounts_and_scopes_by_project_id(
        project_id.clone(),
        &notify_server.postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        subscribers,
        vec![SubscriberAccountAndScopes {
            account: account.clone(),
            scope: scope.into_iter().chain(vec![new_type]).collect(),
        }]
    );
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn test_notify_non_existant_project(notify_server: &NotifyServerContext) {
    let project_id = ProjectId::generate();

    let notification = Notification {
        r#type: Uuid::new_v4(),
        title: "title".to_owned(),
        body: "body".to_owned(),
        icon: Some("icon".to_owned()),
        url: Some("url".to_owned()),
    };

    let notification_body = NotifyBodyNotification {
        notification_id: None,
        notification,
        accounts: vec![generate_account_id()],
    };
    let notify_body = vec![notification_body];

    let notify_url = notify_server
        .url
        .join(&format!("/v1/{project_id}/notify"))
        .unwrap();

    let response = reqwest::Client::new()
        .post(notify_url)
        .bearer_auth(Uuid::new_v4())
        .json(&notify_body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        response
            .json::<Value>()
            .await
            .unwrap()
            .get("error")
            .unwrap()
            .as_str()
            .unwrap(),
        "Project not found"
    );
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn test_notify_invalid_notification_type(notify_server: &NotifyServerContext) {
    let project_id = ProjectId::generate();
    let app_domain = &generate_app_domain();
    let topic = Topic::generate();
    let subscribe_key = generate_subscribe_key();
    let authentication_key = generate_authentication_key();
    upsert_project(
        project_id.clone(),
        app_domain,
        topic,
        &authentication_key,
        &subscribe_key,
        &notify_server.postgres,
        None,
    )
    .await
    .unwrap();

    let notify_body = json!([{
        "notification": {
        "type": "junk",
        "title": "title",
        "body": "body",
        },
        "accounts": []
    }]);

    let notify_url = notify_server
        .url
        .join(&format!("/v1/{project_id}/notify"))
        .unwrap();

    let response = reqwest::Client::new()
        .post(notify_url)
        .bearer_auth(Uuid::new_v4())
        .json(&notify_body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let response = response.text().await.unwrap();
    assert!(response.contains("Failed to deserialize the JSON body into the target type"));
    assert!(response.contains("type: UUID parsing failed"));
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn test_notify_invalid_notification_title(notify_server: &NotifyServerContext) {
    let project_id = ProjectId::generate();
    let app_domain = &generate_app_domain();
    let topic = Topic::generate();
    let subscribe_key = generate_subscribe_key();
    let authentication_key = generate_authentication_key();
    upsert_project(
        project_id.clone(),
        app_domain,
        topic,
        &authentication_key,
        &subscribe_key,
        &notify_server.postgres,
        None,
    )
    .await
    .unwrap();

    let notify_body = json!([{
        "notification": {
        "type": Uuid::new_v4(),
        "title": "",
        "body": "body",
        },
        "accounts": []
    }]);

    let notify_url = notify_server
        .url
        .join(&format!("/v1/{project_id}/notify"))
        .unwrap();

    let response = reqwest::Client::new()
        .post(notify_url)
        .bearer_auth(Uuid::new_v4())
        .json(&notify_body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response = response.text().await.unwrap();
    assert!(response.contains("title: Validation error: length"));
}

#[tokio::test]
async fn test_dead_letter_and_giveup_checks() {
    let (postgres, _) = get_postgres().await;

    // Populating `project`, `subscriber`, `notification` with the data
    let topic = Topic::generate();
    let project_id = ProjectId::generate();
    let subscribe_key = generate_subscribe_key();
    let authentication_key = generate_authentication_key();
    let app_domain = generate_app_domain();
    upsert_project(
        project_id.clone(),
        &app_domain,
        topic,
        &authentication_key,
        &subscribe_key,
        &postgres,
        None,
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &postgres, None)
        .await
        .unwrap();

    let account_id = generate_account_id();
    let subscriber_sym_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let subscriber_topic: Topic = sha256::digest(&subscriber_sym_key).into();
    let subscriber_scope = HashSet::from([Uuid::new_v4(), Uuid::new_v4()]);
    let subscriber_id = upsert_subscriber(
        project.id,
        account_id.clone(),
        subscriber_scope.clone(),
        &subscriber_sym_key,
        subscriber_topic.clone(),
        &postgres,
        None,
    )
    .await
    .unwrap();

    let notification_with_id = upsert_notification(
        "test_notification".to_owned(),
        project.id,
        Notification {
            r#type: Uuid::new_v4(),
            title: "title".to_owned(),
            body: "body".to_owned(),
            icon: None,
            url: None,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();

    // Insert notify for delivery
    upsert_subscriber_notifications(notification_with_id.id, &[subscriber_id], &postgres, None)
        .await
        .unwrap();

    // Get the notify message for processing
    let processing_notify = pick_subscriber_notification_for_processing(&postgres, None)
        .await
        .unwrap();
    assert!(
        processing_notify.is_some(),
        "No notification for processing found"
    );

    // Run dead letter check and try to get another message for processing
    // and expect that there are no messages because the threshold is not reached
    let dead_letter_threshold = std::time::Duration::from_secs(60); // one minute
    dead_letters_check(dead_letter_threshold, &postgres, None)
        .await
        .unwrap();
    assert!(
        pick_subscriber_notification_for_processing(&postgres, None)
            .await
            .unwrap()
            .is_none(),
        "The messages should be already in the processing state and should not be picked"
    );

    // Manually change the `updated_at` for the notify message to be older than the
    // dead letter threshold  plus 10 seconds
    let NotificationToProcess {
        subscriber_notification: subscriber_notification_id,
        notification_created_at,
        ..
    } = processing_notify.unwrap();
    let query = "UPDATE subscriber_notification SET updated_at = $1 WHERE id = $2";
    sqlx::query::<Postgres>(query)
        .bind(Utc::now() - dead_letter_threshold - Duration::seconds(10))
        .bind(subscriber_notification_id)
        .execute(&postgres)
        .await
        .unwrap();

    // Start to listen for pg_notify for the dead letters put back into the processing queue
    let mut pg_listener = sqlx::postgres::PgListener::connect_with(&postgres)
        .await
        .unwrap();
    pg_listener
        .listen("notification_for_delivery")
        .await
        .unwrap();
    // Spawn a new tokio task for listener
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        pg_listener.recv().await.unwrap();
        tx.send(()).unwrap();
    });

    // Run dead letter checks to put the message back into the processing queue
    dead_letters_check(dead_letter_threshold, &postgres, None)
        .await
        .unwrap();

    // Setting a timeout of 3 seconds to wait for pg_notify notifications
    assert!(
        tokio::time::timeout(tokio::time::Duration::from_secs(3), rx)
            .await
            .is_ok(),
        "Timeout waiting for the pg_notify is reached"
    );

    // Get the notify message for processing after dead letter check put it back
    let processing_notify = pick_subscriber_notification_for_processing(&postgres, None)
        .await
        .unwrap();
    assert!(
        processing_notify.is_some(),
        "No notification for processing found after the dead letter check"
    );
    assert_eq!(
        processing_notify.unwrap().subscriber_notification,
        subscriber_notification_id
    );

    // Manually updating `created_at` for the notify message to be older than the
    // give up letter processing threshold plus 10 seconds
    let give_up_threshold = std::time::Duration::from_secs(60); // one minute

    let give_up_result_before =
        dead_letter_give_up_check(notification_created_at, give_up_threshold);
    assert!(!give_up_result_before);

    let notification_created_at = Utc::now() - give_up_threshold - Duration::seconds(10);

    let give_up_result_after =
        dead_letter_give_up_check(notification_created_at, give_up_threshold);
    assert!(give_up_result_after);
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn test_subscribe_topic(notify_server: &NotifyServerContext) {
    let project_id = ProjectId::generate();
    let app_domain = &generate_app_domain();
    let subscribe_topic_response = reqwest::Client::new()
        .post(
            notify_server
                .url
                .join(&format!("/{project_id}/subscribe-topic",))
                .unwrap(),
        )
        .bearer_auth(Uuid::new_v4())
        .json(&SubscribeTopicRequestBody {
            app_domain: app_domain.to_owned(),
        })
        .send()
        .await
        .unwrap();
    assert_eq!(subscribe_topic_response.status(), StatusCode::OK);

    let response = subscribe_topic_response
        .json::<SubscribeTopicResponseBody>()
        .await
        .unwrap();

    let project = get_project_by_project_id(project_id.clone(), &notify_server.postgres, None)
        .await
        .unwrap();
    assert_eq!(project.subscribe_public_key, response.subscribe_key);
    assert_eq!(
        project.authentication_public_key,
        response.authentication_key
    );
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn test_subscribe_topic_idempotency(notify_server: &NotifyServerContext) {
    let project_id = ProjectId::generate();
    let app_domain = &generate_app_domain();

    let subscribe_topic_response = reqwest::Client::new()
        .post(
            notify_server
                .url
                .join(&format!("/{project_id}/subscribe-topic",))
                .unwrap(),
        )
        .bearer_auth(Uuid::new_v4())
        .json(&SubscribeTopicRequestBody {
            app_domain: app_domain.to_owned(),
        })
        .send()
        .await
        .unwrap();
    assert_eq!(subscribe_topic_response.status(), StatusCode::OK);

    let subscribe_topic_response = reqwest::Client::new()
        .post(
            notify_server
                .url
                .join(&format!("/{project_id}/subscribe-topic",))
                .unwrap(),
        )
        .bearer_auth(Uuid::new_v4())
        .json(&SubscribeTopicRequestBody {
            app_domain: app_domain.to_owned(),
        })
        .send()
        .await
        .unwrap();
    assert_eq!(subscribe_topic_response.status(), StatusCode::OK);
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn test_subscribe_topic_conflict(notify_server: &NotifyServerContext) {
    let project_id = ProjectId::generate();
    let app_domain = &generate_app_domain();

    let subscribe_topic_response = reqwest::Client::new()
        .post(
            notify_server
                .url
                .join(&format!("/{project_id}/subscribe-topic",))
                .unwrap(),
        )
        .bearer_auth(Uuid::new_v4())
        .json(&SubscribeTopicRequestBody {
            app_domain: app_domain.to_owned(),
        })
        .send()
        .await
        .unwrap();
    assert_eq!(subscribe_topic_response.status(), StatusCode::OK);

    let project_id = ProjectId::generate();
    let subscribe_topic_response = reqwest::Client::new()
        .post(
            notify_server
                .url
                .join(&format!("/{project_id}/subscribe-topic",))
                .unwrap(),
        )
        .bearer_auth(Uuid::new_v4())
        .json(&SubscribeTopicRequestBody {
            app_domain: app_domain.to_owned(),
        })
        .send()
        .await
        .unwrap();
    assert_eq!(subscribe_topic_response.status(), StatusCode::CONFLICT);
}

async fn get_notify_did_json(
    notify_server_url: &Url,
) -> (x25519_dalek::PublicKey, DecodedClientId) {
    let did_json_url = notify_server_url.join(DID_JSON_ENDPOINT).unwrap();
    let did_json = reqwest::get(did_json_url)
        .await
        .unwrap()
        .json::<DidJson>()
        .await
        .unwrap();
    let key_agreement = &did_json
        .verification_method
        .iter()
        .find(|key| key.id.ends_with(WC_NOTIFY_SUBSCRIBE_KEY_ID))
        .unwrap()
        .public_key_jwk
        .x;
    let authentication = &did_json
        .verification_method
        .iter()
        .find(|key| key.id.ends_with(WC_NOTIFY_AUTHENTICATION_KEY_ID))
        .unwrap()
        .public_key_jwk
        .x;
    let key_agreement: [u8; 32] = BASE64URL
        .decode(key_agreement.as_bytes())
        .unwrap()
        .try_into()
        .unwrap();
    let authentication: [u8; 32] = BASE64URL
        .decode(authentication.as_bytes())
        .unwrap()
        .try_into()
        .unwrap();
    (
        x25519_dalek::PublicKey::from(key_agreement),
        // Better approach, but dependency versions conflict right now
        // DecodedClientId::from_key(
        //     ed25519_dalek::VerifyingKey::from_bytes(&authentication).unwrap(),
        // ),
        DecodedClientId(authentication),
    )
}

fn topic_from_key(key: &[u8]) -> Topic {
    sha256::digest(key).into()
}

enum MessageMethod {
    WatchSubscriptionsRequest,
}

struct IdentityKeyDetails<'a> {
    keys_server_url: &'a Url,
    signing_key: &'a SigningKey,
    did_key: &'a str,
}

// TODO move to method
enum TopicEncryptionScheme {
    // Symetric([u8; 32]),
    Asymetric {
        client_private: x25519_dalek::StaticSecret,
        client_public: x25519_dalek::PublicKey,
        server_public: x25519_dalek::PublicKey,
    },
}

async fn publish_jwt_message<'a>(
    relay_ws_client: &Client,
    app_domain: Option<&str>,
    did_pkh: String,
    aud_authentication_key: &DecodedClientId,
    identity_key_details: IdentityKeyDetails<'a>,
    method: MessageMethod,
    encryption_details: TopicEncryptionScheme,
) {
    let (method, tag, ttl, act) = match method {
        MessageMethod::WatchSubscriptionsRequest => (
            NOTIFY_WATCH_SUBSCRIPTIONS_METHOD,
            NOTIFY_WATCH_SUBSCRIPTIONS_TAG,
            NOTIFY_WATCH_SUBSCRIPTIONS_TTL,
            NOTIFY_WATCH_SUBSCRIPTIONS_ACT,
        ),
    };

    let now = Utc::now();
    let subscription_auth = WatchSubscriptionsRequestAuth {
        shared_claims: SharedClaims {
            iat: now.timestamp() as u64,
            exp: add_ttl(now, ttl).timestamp() as u64,
            iss: identity_key_details.did_key.to_owned(),
            act: act.to_owned(),
            aud: aud_authentication_key.to_did_key(),
            mjv: "0".to_owned(),
        },
        ksu: identity_key_details.keys_server_url.to_string(),
        sub: did_pkh,
        app: app_domain.map(|domain| DidWeb::from_domain(domain.to_owned())),
    };

    let message = NotifyRequest::new(
        method,
        NotifyWatchSubscriptions {
            watch_subscriptions_auth: encode_auth(
                &subscription_auth,
                identity_key_details.signing_key,
            ),
        },
    );

    let (envelope, topic_key) = match encryption_details {
        TopicEncryptionScheme::Asymetric {
            client_private: client_secret,
            client_public,
            server_public,
        } => {
            let response_topic_key = derive_key(&server_public, &client_secret).unwrap();
            (
                Envelope::<EnvelopeType1>::new(
                    &response_topic_key,
                    message,
                    *client_public.as_bytes(),
                )
                .unwrap(),
                server_public,
            )
        }
    };
    let message = BASE64.encode(envelope.to_bytes());

    let topic = topic_from_key(topic_key.as_bytes());
    relay_ws_client
        .publish(topic, message, tag, ttl, false)
        .await
        .unwrap();
}

#[allow(clippy::too_many_arguments)]
async fn watch_subscriptions(
    notify_server_url: Url,
    keys_server_url: Url,
    app_domain: Option<&str>,
    identity_signing_key: &SigningKey,
    identity_did_key: &str,
    did_pkh: &str,
    relay_ws_client: &relay_client::websocket::Client,
    rx: &mut UnboundedReceiver<RelayClientEvent>,
) -> (Vec<NotifyServerSubscription>, [u8; 32]) {
    let (key_agreement_key, authentication_key) = get_notify_did_json(&notify_server_url).await;

    let secret = StaticSecret::random_from_rng(OsRng);
    let public = PublicKey::from(&secret);

    let response_topic_key = derive_key(&key_agreement_key, &secret).unwrap();
    let response_topic = topic_from_key(&response_topic_key);

    publish_jwt_message(
        relay_ws_client,
        app_domain,
        did_pkh.to_owned(),
        &authentication_key,
        IdentityKeyDetails {
            keys_server_url: &keys_server_url,
            signing_key: identity_signing_key,
            did_key: identity_did_key,
        },
        MessageMethod::WatchSubscriptionsRequest,
        TopicEncryptionScheme::Asymetric {
            client_private: secret,
            client_public: public,
            server_public: key_agreement_key,
        },
    )
    .await;

    relay_ws_client
        .subscribe(response_topic.clone())
        .await
        .unwrap();

    async fn accept_message(rx: &mut UnboundedReceiver<RelayClientEvent>) -> PublishedMessage {
        let event = rx.recv().await.unwrap();
        match event {
            RelayClientEvent::Message(msg) => msg,
            e => panic!("Expected message, got {e:?}"),
        }
    }

    let msg = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let msg = accept_message(rx).await;
            if msg.tag == NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_TAG && msg.topic == response_topic {
                return msg;
            }
        }
    })
    .await
    .unwrap();

    let Envelope::<EnvelopeType0> { sealbox, iv, .. } =
        Envelope::<EnvelopeType0>::from_bytes(BASE64.decode(msg.message.as_bytes()).unwrap())
            .unwrap();
    let decrypted_response = ChaCha20Poly1305::new(GenericArray::from_slice(&response_topic_key))
        .decrypt(&iv.into(), chacha20poly1305::aead::Payload::from(&*sealbox))
        .unwrap();
    let response =
        serde_json::from_slice::<NotifyResponse<ResponseAuth>>(&decrypted_response).unwrap();

    let auth = from_jwt::<WatchSubscriptionsResponseAuth>(&response.result.response_auth).unwrap();
    assert_eq!(
        auth.shared_claims.act,
        NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_ACT
    );
    assert_eq!(auth.shared_claims.iss, authentication_key.to_did_key());

    (auth.sbs, response_topic_key)
}

fn generate_identity_key() -> (SigningKey, DecodedClientId) {
    let keypair = Keypair::generate(&mut StdRng::from_entropy());
    let signing_key = SigningKey::from_bytes(keypair.secret_key().as_bytes());
    let client_id = DecodedClientId::from_key(&keypair.public_key());
    (signing_key, client_id)
}

fn sign_cacao(
    app_domain: String,
    did_pkh: String,
    statement: String,
    identity_public_key: DecodedClientId,
    keys_server_url: String,
    account_signing_key: k256::ecdsa::SigningKey,
) -> cacao::Cacao {
    let mut cacao = cacao::Cacao {
        h: cacao::header::Header {
            t: EIP4361.to_owned(),
        },
        p: cacao::payload::Payload {
            domain: app_domain,
            iss: did_pkh,
            statement: Some(statement),
            aud: identity_public_key.to_did_key(),
            version: cacao::Version::V1,
            nonce: hex::encode(rand::Rng::gen::<[u8; 10]>(&mut rand::thread_rng())),
            iat: Utc::now().to_rfc3339(),
            exp: None,
            nbf: None,
            request_id: None,
            resources: Some(vec![keys_server_url]),
        },
        s: cacao::signature::Signature {
            t: "".to_owned(),
            s: "".to_owned(),
        },
    };
    let (signature, recovery): (k256::ecdsa::Signature, _) = account_signing_key
        .sign_digest_recoverable(Keccak256::new_with_prefix(
            Eip191.eip191_bytes(&cacao.siwe_message().unwrap()),
        ))
        .unwrap();
    let cacao_signature = [&signature.to_bytes()[..], &[recovery.to_byte()]].concat();
    cacao.s.t = EIP191.to_owned();
    cacao.s.s = hex::encode(cacao_signature);
    cacao.verify().unwrap();
    cacao
}

async fn subscribe_topic(
    project_id: &ProjectId,
    app_domain: String,
    notify_server_url: &Url,
) -> SubscribeTopicResponseBody {
    assert_successful_response(
        reqwest::Client::new()
            .post(
                notify_server_url
                    .join(&format!("/{project_id}/subscribe-topic",))
                    .unwrap(),
            )
            .bearer_auth(Uuid::new_v4())
            .json(&SubscribeTopicRequestBody { app_domain })
            .send()
            .await
            .unwrap(),
    )
    .await
    .json::<SubscribeTopicResponseBody>()
    .await
    .unwrap()
}

async fn run_test(
    statement: String,
    watch_subscriptions_all_domains: bool,
    notify_server: &NotifyServerContext,
) {
    let keys_server_url = "https://staging.keys.walletconnect.com"
        .parse::<Url>()
        .unwrap();

    let (identity_signing_key, identity_public_key) = generate_identity_key();
    let identity_did_key = identity_public_key.to_did_key();

    let (account_signing_key, account) = generate_account();
    let did_pkh = account.to_did_pkh();

    let project_id = ProjectId::generate();
    let app_domain = format!("{project_id}.walletconnect.com");

    assert_successful_response(
        reqwest::Client::builder()
            .build()
            .unwrap()
            .post(keys_server_url.join("/identity").unwrap())
            .json(&CacaoValue {
                cacao: sign_cacao(
                    app_domain.clone(),
                    did_pkh.clone(),
                    statement,
                    identity_public_key.clone(),
                    keys_server_url.to_string(),
                    account_signing_key,
                ),
            })
            .send()
            .await
            .unwrap(),
    )
    .await;

    // ==== watchSubscriptions ====
    // {
    //     let (relay_ws_client, mut rx) = create_client(&relay_url, &relay_project_id, &notify_url).await;

    //     let (subs, _) = watch_subscriptions(
    //         app_domain,
    //         &notify_url,
    //         &identity_signing_key,
    //         &identity_did_key,
    //         &did_pkh,
    //         &relay_ws_client,
    //         &mut rx,
    //     )
    //     .await;

    //     assert!(subs.is_empty());
    // }

    let vars = get_vars();
    let (relay_ws_client, mut rx) = create_client(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    let subscribe_topic_response_body =
        subscribe_topic(&project_id, app_domain.clone(), &notify_server.url).await;

    let watch_topic_key = {
        let (subs, watch_topic_key) = watch_subscriptions(
            notify_server.url.clone(),
            keys_server_url.clone(),
            if watch_subscriptions_all_domains {
                None
            } else {
                Some(&app_domain)
            },
            &identity_signing_key,
            &identity_did_key,
            &did_pkh,
            &relay_ws_client,
            &mut rx,
        )
        .await;

        assert!(subs.is_empty());

        watch_topic_key
    };

    let app_subscribe_public_key = &subscribe_topic_response_body.subscribe_key;
    let app_authentication_public_key = &subscribe_topic_response_body.authentication_key;
    let dapp_did_key = DecodedClientId(
        hex::decode(app_authentication_public_key)
            .unwrap()
            .as_slice()
            .try_into()
            .unwrap(),
    )
    .to_did_key();

    // Get subscribe topic for dapp
    let subscribe_topic = sha256::digest(hex::decode(app_subscribe_public_key).unwrap().as_slice());

    // ----------------------------------------------------
    // SUBSCRIBE WALLET CLIENT TO DAPP THROUGHT NOTIFY
    // ----------------------------------------------------

    // Prepare subscription auth for *wallet* client
    // https://github.com/WalletConnect/walletconnect-docs/blob/main/docs/specs/clients/notify/notify-authentication.md#notify-subscription
    let notification_type = Uuid::new_v4();
    let notification_types = HashSet::from([notification_type, Uuid::new_v4()]);
    let now = Utc::now();
    let subscription_auth = SubscriptionRequestAuth {
        shared_claims: SharedClaims {
            iat: now.timestamp() as u64,
            exp: add_ttl(now, NOTIFY_SUBSCRIBE_TTL).timestamp() as u64,
            iss: identity_did_key.clone(),
            act: "notify_subscription".to_owned(),
            aud: dapp_did_key.clone(),
            mjv: "0".to_owned(),
        },
        ksu: keys_server_url.to_string(),
        sub: did_pkh.clone(),
        scp: notification_types
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" "),
        app: DidWeb::from_domain(app_domain.clone()),
    };

    // Encode the subscription auth
    let subscription_auth = encode_auth(&subscription_auth, &identity_signing_key);

    let sub_auth = json!({ "subscriptionAuth": subscription_auth });
    let message = NotifyRequest::new(NOTIFY_SUBSCRIBE_METHOD, sub_auth);

    let subscription_secret = StaticSecret::random_from_rng(OsRng);
    let subscription_public = PublicKey::from(&subscription_secret);
    let response_topic_key = derive_key(
        &x25519_dalek::PublicKey::from(decode_key(app_subscribe_public_key).unwrap()),
        &subscription_secret,
    )
    .unwrap();

    let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(&response_topic_key));

    let envelope: Envelope<EnvelopeType1> = Envelope::<EnvelopeType1>::new(
        &response_topic_key,
        message,
        *subscription_public.as_bytes(),
    )
    .unwrap();
    let message = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    // Get response topic for wallet client and notify communication
    let response_topic = sha256::digest(&response_topic_key);
    println!("subscription response_topic: {response_topic}");

    // Subscribe to the topic and listen for response
    relay_ws_client
        .subscribe(response_topic.clone().into())
        .await
        .unwrap();

    // Send subscription request to notify
    relay_ws_client
        .publish(
            subscribe_topic.into(),
            message,
            NOTIFY_SUBSCRIBE_TAG,
            NOTIFY_SUBSCRIBE_TTL,
            false,
        )
        .await
        .unwrap();

    let resp = rx.recv().await.unwrap();
    let RelayClientEvent::Message(msg) = resp else {
        panic!("Expected message, got {:?}", resp);
    };
    let msg = if msg.tag == NOTIFY_SUBSCRIBE_RESPONSE_TAG {
        assert_eq!(msg.tag, NOTIFY_SUBSCRIBE_RESPONSE_TAG);
        msg
    } else {
        println!(
            "got additional message with unexpected tag {} msg.id {} and message_id {}",
            msg.tag,
            msg.message_id,
            sha256::digest(msg.message.as_ref()),
        );
        let Envelope::<EnvelopeType0> { sealbox, iv, .. } = Envelope::<EnvelopeType0>::from_bytes(
            base64::engine::general_purpose::STANDARD
                .decode(msg.message.as_bytes())
                .unwrap(),
        )
        .unwrap();
        let decrypted_response = ChaCha20Poly1305::new(GenericArray::from_slice(&watch_topic_key))
            .decrypt(&iv.into(), chacha20poly1305::aead::Payload::from(&*sealbox))
            .unwrap();
        let response: NotifyResponse<serde_json::Value> =
            serde_json::from_slice(&decrypted_response).unwrap();
        println!(
            "warn: got additional message with unexpected tag {} msg.id {} and message_id {} RPC ID {}",
            msg.tag,
            msg.message_id,
            sha256::digest(msg.message.as_ref()),
            response.id,
        );

        let resp = rx.recv().await.unwrap();
        let RelayClientEvent::Message(msg) = resp else {
            panic!("Expected message, got {:?}", resp);
        };
        assert_eq!(msg.tag, NOTIFY_SUBSCRIBE_RESPONSE_TAG);
        msg
    };

    let Envelope::<EnvelopeType0> { sealbox, iv, .. } = Envelope::<EnvelopeType0>::from_bytes(
        base64::engine::general_purpose::STANDARD
            .decode(msg.message.as_bytes())
            .unwrap(),
    )
    .unwrap();

    let decrypted_response = cipher
        .decrypt(&iv.into(), chacha20poly1305::aead::Payload::from(&*sealbox))
        .unwrap();

    let response: NotifyResponse<serde_json::Value> =
        serde_json::from_slice(&decrypted_response).unwrap();

    let response_auth = response
        .result
        .get("responseAuth") // TODO use structure
        .unwrap()
        .as_str()
        .unwrap();
    let subscribe_response_auth = from_jwt::<SubscriptionResponseAuth>(response_auth).unwrap();
    assert_eq!(
        subscribe_response_auth.shared_claims.act,
        "notify_subscription_response"
    );

    let notify_key = {
        let resp = rx.recv().await.unwrap();

        let RelayClientEvent::Message(msg) = resp else {
            panic!("Expected message, got {:?}", resp);
        };
        assert_eq!(msg.tag, NOTIFY_SUBSCRIPTIONS_CHANGED_TAG);

        let Envelope::<EnvelopeType0> { sealbox, iv, .. } = Envelope::<EnvelopeType0>::from_bytes(
            base64::engine::general_purpose::STANDARD
                .decode(msg.message.as_bytes())
                .unwrap(),
        )
        .unwrap();

        let decrypted_response = ChaCha20Poly1305::new(GenericArray::from_slice(&watch_topic_key))
            .decrypt(&iv.into(), chacha20poly1305::aead::Payload::from(&*sealbox))
            .unwrap();

        let response: NotifyRequest<serde_json::Value> =
            serde_json::from_slice(&decrypted_response).unwrap();

        let response_auth = response
            .params
            .get("subscriptionsChangedAuth") // TODO use structure
            .unwrap()
            .as_str()
            .unwrap();
        let auth = from_jwt::<WatchSubscriptionsChangedRequestAuth>(response_auth).unwrap();
        assert_eq!(auth.shared_claims.act, "notify_subscriptions_changed");
        assert_eq!(auth.sbs.len(), 1);
        let sub = &auth.sbs[0];
        assert_eq!(sub.scope, notification_types);
        assert_eq!(sub.account, account);
        assert_eq!(sub.app_domain, app_domain);
        assert_eq!(&sub.app_authentication_key, &dapp_did_key);
        assert_eq!(
            DecodedClientId::try_from_did_key(&sub.app_authentication_key)
                .unwrap()
                .0,
            decode_key(app_authentication_public_key).unwrap()
        );
        assert_eq!(sub.scope, notification_types);
        decode_key(&sub.sym_key).unwrap()
    };

    let notify_topic = sha256::digest(&notify_key);

    relay_ws_client
        .subscribe(notify_topic.clone().into())
        .await
        .unwrap();

    let msg_4050 = rx.recv().await.unwrap();
    let RelayClientEvent::Message(msg) = msg_4050 else {
        panic!("Expected message, got {:?}", msg_4050);
    };
    assert_eq!(msg.tag, NOTIFY_NOOP_TAG);

    let notification = Notification {
        r#type: notification_type,
        title: "title".to_owned(),
        body: "body".to_owned(),
        icon: Some("icon".to_owned()),
        url: Some("url".to_owned()),
    };

    let notify_body = NotifyBody {
        notification_id: None,
        notification: notification.clone(),
        accounts: vec![account.clone()],
    };

    // wait for notify server to register the user
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    let _res = reqwest::Client::new()
        .post(
            notify_server
                .url
                .join(&format!("/{project_id}/notify",))
                .unwrap(),
        )
        .bearer_auth(Uuid::new_v4())
        .json(&notify_body)
        .send()
        .await
        .unwrap();

    let resp = rx.recv().await.unwrap();
    let RelayClientEvent::Message(msg) = resp else {
        panic!("Expected message, got {:?}", resp);
    };
    assert_eq!(msg.tag, NOTIFY_MESSAGE_TAG);

    let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(&notify_key));

    let Envelope::<EnvelopeType0> { iv, sealbox, .. } = Envelope::<EnvelopeType0>::from_bytes(
        base64::engine::general_purpose::STANDARD
            .decode(msg.message.as_bytes())
            .unwrap(),
    )
    .unwrap();

    // TODO: add proper type for that val
    let decrypted_notification: NotifyRequest<NotifyPayload> = serde_json::from_slice(
        &cipher
            .decrypt(&iv.into(), chacha20poly1305::aead::Payload::from(&*sealbox))
            .unwrap(),
    )
    .unwrap();

    // let received_notification = decrypted_notification.params;
    let claims = verify_jwt(
        &decrypted_notification.params.message_auth,
        &decode_authentication_public_key(app_authentication_public_key),
    )
    .unwrap();

    // https://github.com/WalletConnect/walletconnect-docs/blob/main/docs/specs/clients/notify/notify-authentication.md#notify-message
    // TODO: verify issuer
    assert_eq!(claims.msg.r#type, notification.r#type);
    assert_eq!(claims.msg.title, notification.title);
    assert_eq!(claims.msg.body, notification.body);
    assert_eq!(claims.msg.icon, "icon");
    assert_eq!(claims.msg.url, "url");
    assert_eq!(claims.sub, did_pkh);
    assert!(claims.iat < chrono::Utc::now().timestamp() + JWT_LEEWAY); // TODO remove leeway
    assert!(claims.exp > chrono::Utc::now().timestamp() - JWT_LEEWAY); // TODO remove leeway
    assert_eq!(claims.app.as_ref(), app_domain);
    assert_eq!(claims.sub, did_pkh);
    assert_eq!(claims.act, "notify_message");

    // TODO Notify receipt?
    // https://github.com/WalletConnect/walletconnect-docs/blob/main/docs/specs/clients/notify/notify-authentication.md#notify-receipt

    // Update subscription

    // Prepare update auth for *wallet* client
    // https://github.com/WalletConnect/walletconnect-docs/blob/main/docs/specs/clients/notify/notify-authentication.md#notify-updatelet notification_type = Uuid::new_v4();
    let notification_type = Uuid::new_v4();
    let notification_types = HashSet::from([notification_type, Uuid::new_v4(), Uuid::new_v4()]);
    let now = Utc::now();
    let update_auth = SubscriptionUpdateRequestAuth {
        shared_claims: SharedClaims {
            iat: now.timestamp() as u64,
            exp: add_ttl(now, NOTIFY_UPDATE_TTL).timestamp() as u64,
            iss: identity_did_key.clone(),
            act: "notify_update".to_owned(),
            aud: dapp_did_key.clone(),
            mjv: "0".to_owned(),
        },
        ksu: keys_server_url.to_string(),
        sub: did_pkh.clone(),
        scp: notification_types
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" "),
        app: DidWeb::from_domain(app_domain.clone()),
    };

    // Encode the subscription auth
    let update_auth = encode_auth(&update_auth, &identity_signing_key);

    let sub_auth = json!({ "updateAuth": update_auth });

    let delete_message = NotifyRequest::new(NOTIFY_UPDATE_METHOD, sub_auth);

    let envelope = Envelope::<EnvelopeType0>::new(&notify_key, delete_message).unwrap();

    let encoded_message = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    relay_ws_client
        .publish(
            notify_topic.clone().into(),
            encoded_message,
            NOTIFY_UPDATE_TAG,
            NOTIFY_UPDATE_TTL,
            false,
        )
        .await
        .unwrap();

    // Check for update response
    let resp = rx.recv().await.unwrap();

    let RelayClientEvent::Message(msg) = resp else {
        panic!("Expected message, got {:?}", resp);
    };
    assert_eq!(msg.tag, NOTIFY_UPDATE_RESPONSE_TAG);

    let Envelope::<EnvelopeType0> { sealbox, iv, .. } = Envelope::<EnvelopeType0>::from_bytes(
        base64::engine::general_purpose::STANDARD
            .decode(msg.message.as_bytes())
            .unwrap(),
    )
    .unwrap();

    let decrypted_response = cipher
        .decrypt(&iv.into(), chacha20poly1305::aead::Payload::from(&*sealbox))
        .unwrap();

    let response: NotifyResponse<serde_json::Value> =
        serde_json::from_slice(&decrypted_response).unwrap();

    let response_auth = response
        .result
        .get("responseAuth") // TODO use structure
        .unwrap()
        .as_str()
        .unwrap();
    let claims = from_jwt::<SubscriptionUpdateResponseAuth>(response_auth).unwrap();
    // https://github.com/WalletConnect/walletconnect-docs/blob/main/docs/specs/clients/notify/notify-authentication.md#notify-update-response
    // TODO verify issuer
    assert_eq!(claims.sub, did_pkh);
    assert!((claims.shared_claims.iat as i64) < chrono::Utc::now().timestamp() + JWT_LEEWAY); // TODO remove leeway
    assert!((claims.shared_claims.exp as i64) > chrono::Utc::now().timestamp() - JWT_LEEWAY); // TODO remove leeway
    assert_eq!(claims.app, DidWeb::from_domain(app_domain.clone()));
    assert_eq!(claims.shared_claims.aud, identity_did_key);
    assert_eq!(claims.shared_claims.act, "notify_update_response");

    {
        let resp = rx.recv().await.unwrap();

        let RelayClientEvent::Message(msg) = resp else {
            panic!("Expected message, got {:?}", resp);
        };
        assert_eq!(msg.tag, NOTIFY_SUBSCRIPTIONS_CHANGED_TAG);

        let Envelope::<EnvelopeType0> { sealbox, iv, .. } = Envelope::<EnvelopeType0>::from_bytes(
            base64::engine::general_purpose::STANDARD
                .decode(msg.message.as_bytes())
                .unwrap(),
        )
        .unwrap();

        let decrypted_response = ChaCha20Poly1305::new(GenericArray::from_slice(&watch_topic_key))
            .decrypt(&iv.into(), chacha20poly1305::aead::Payload::from(&*sealbox))
            .unwrap();

        let response: NotifyRequest<serde_json::Value> =
            serde_json::from_slice(&decrypted_response).unwrap();

        let response_auth = response
            .params
            .get("subscriptionsChangedAuth") // TODO use structure
            .unwrap()
            .as_str()
            .unwrap();
        let auth = from_jwt::<WatchSubscriptionsChangedRequestAuth>(response_auth).unwrap();
        assert_eq!(auth.shared_claims.act, "notify_subscriptions_changed");
        assert_eq!(auth.sbs.len(), 1);
        let subs = &auth.sbs[0];
        assert_eq!(subs.scope, notification_types);
    }

    // Prepare deletion auth for *wallet* client
    // https://github.com/WalletConnect/walletconnect-docs/blob/main/docs/specs/clients/notify/notify-authentication.md#notify-delete
    let now = Utc::now();
    let delete_auth = SubscriptionDeleteRequestAuth {
        shared_claims: SharedClaims {
            iat: now.timestamp() as u64,
            exp: add_ttl(now, NOTIFY_DELETE_TTL).timestamp() as u64,
            iss: identity_did_key.clone(),
            aud: dapp_did_key.clone(),
            act: "notify_delete".to_owned(),
            mjv: "0".to_owned(),
        },
        ksu: keys_server_url.to_string(),
        sub: did_pkh.clone(),
        app: DidWeb::from_domain(app_domain.clone()),
    };

    // Encode the subscription auth
    let delete_auth = encode_auth(&delete_auth, &identity_signing_key);
    let _delete_auth_hash = sha256::digest(&*delete_auth.clone());

    let sub_auth = json!({ "deleteAuth": delete_auth });

    let delete_message = NotifyRequest::new(NOTIFY_DELETE_METHOD, sub_auth);

    let envelope = Envelope::<EnvelopeType0>::new(&notify_key, delete_message).unwrap();

    let encoded_message = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    relay_ws_client
        .publish(
            notify_topic.into(),
            encoded_message,
            NOTIFY_DELETE_TAG,
            NOTIFY_DELETE_TTL,
            false,
        )
        .await
        .unwrap();

    // Check for delete response
    let resp = rx.recv().await.unwrap();

    let RelayClientEvent::Message(msg) = resp else {
        panic!("Expected message, got {:?}", resp);
    };
    assert_eq!(msg.tag, NOTIFY_DELETE_RESPONSE_TAG);

    let Envelope::<EnvelopeType0> { sealbox, iv, .. } = Envelope::<EnvelopeType0>::from_bytes(
        base64::engine::general_purpose::STANDARD
            .decode(msg.message.as_bytes())
            .unwrap(),
    )
    .unwrap();

    let decrypted_response = cipher
        .decrypt(&iv.into(), chacha20poly1305::aead::Payload::from(&*sealbox))
        .unwrap();

    let response: NotifyResponse<serde_json::Value> =
        serde_json::from_slice(&decrypted_response).unwrap();

    let response_auth = response
        .result
        .get("responseAuth") // TODO use structure
        .unwrap()
        .as_str()
        .unwrap();
    let claims = from_jwt::<SubscriptionDeleteResponseAuth>(response_auth).unwrap();
    // https://github.com/WalletConnect/walletconnect-docs/blob/main/docs/specs/clients/notify/notify-authentication.md#notify-delete-response
    // TODO verify issuer
    assert_eq!(claims.sub, did_pkh);
    assert!((claims.shared_claims.iat as i64) < chrono::Utc::now().timestamp() + JWT_LEEWAY); // TODO remove leeway
    assert!((claims.shared_claims.exp as i64) > chrono::Utc::now().timestamp() - JWT_LEEWAY); // TODO remove leeway
    assert_eq!(claims.app, DidWeb::from_domain(app_domain));
    assert_eq!(claims.shared_claims.aud, identity_did_key);
    assert_eq!(claims.shared_claims.act, "notify_delete_response");

    {
        let resp = rx.recv().await.unwrap();

        let RelayClientEvent::Message(msg) = resp else {
            panic!("Expected message, got {:?}", resp);
        };
        assert_eq!(msg.tag, NOTIFY_SUBSCRIPTIONS_CHANGED_TAG);

        let Envelope::<EnvelopeType0> { sealbox, iv, .. } = Envelope::<EnvelopeType0>::from_bytes(
            base64::engine::general_purpose::STANDARD
                .decode(msg.message.as_bytes())
                .unwrap(),
        )
        .unwrap();

        let decrypted_response = ChaCha20Poly1305::new(GenericArray::from_slice(&watch_topic_key))
            .decrypt(&iv.into(), chacha20poly1305::aead::Payload::from(&*sealbox))
            .unwrap();

        let response: NotifyRequest<serde_json::Value> =
            serde_json::from_slice(&decrypted_response).unwrap();

        let response_auth = response
            .params
            .get("subscriptionsChangedAuth") // TODO use structure
            .unwrap()
            .as_str()
            .unwrap();
        let auth = from_jwt::<WatchSubscriptionsChangedRequestAuth>(response_auth).unwrap();
        assert_eq!(auth.shared_claims.act, "notify_subscriptions_changed");
        assert!(auth.sbs.is_empty());
    }

    // wait for notify server to unregister the user
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    let resp = reqwest::Client::new()
        .post(
            notify_server
                .url
                .join(&format!("/{project_id}/notify",))
                .unwrap(),
        )
        .bearer_auth(Uuid::new_v4())
        .json(&notify_body)
        .send()
        .await
        .unwrap();

    let resp = resp
        .json::<notify_server::services::public_http_server::handlers::notify_v0::Response>()
        .await
        .unwrap();

    assert_eq!(resp.not_found.len(), 1);

    unregister_identity_key(
        keys_server_url,
        account.to_did_pkh(),
        &identity_signing_key,
        identity_public_key.to_did_key(),
    )
    .await;

    if let Ok(resp) = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv()).await {
        let resp = resp.unwrap();
        let RelayClientEvent::Message(msg) = resp else {
            panic!("Expected message, got {:?}", resp);
        };
        println!(
            "warn: received extra left-over message with tag {}",
            msg.tag
        );
    }
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn notify_all_domains_old(notify_server: &NotifyServerContext) {
    run_test(STATEMENT_ALL_DOMAINS.to_owned(), true, notify_server).await
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn notify_this_domain(notify_server: &NotifyServerContext) {
    run_test(STATEMENT_THIS_DOMAIN.to_owned(), false, notify_server).await
}

async fn register_mocked_identity_key(
    mock_keys_server: &MockServer,
    identity_public_key: DecodedClientId,
    cacao: Cacao,
) {
    Mock::given(method(Method::Get))
        .and(path(KEYS_SERVER_IDENTITY_ENDPOINT))
        .and(query_param(
            KEYS_SERVER_IDENTITY_ENDPOINT_PUBLIC_KEY_QUERY,
            identity_public_key.to_string(),
        ))
        .respond_with(
            ResponseTemplate::new(StatusCode::OK).set_body_json(KeyServerResponse {
                status: KEYS_SERVER_STATUS_SUCCESS.to_owned(),
                error: None,
                value: Some(CacaoValue { cacao }),
            }),
        )
        .mount(mock_keys_server)
        .await;
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn notify_all_domains(notify_server: &NotifyServerContext) {
    let (identity_signing_key, identity_public_key) = generate_identity_key();

    let (account_signing_key, account) = generate_account();
    let did_pkh = account.to_did_pkh();

    let mock_keys_server = MockServer::start().await;
    let mock_keys_server_url = mock_keys_server.uri().parse::<Url>().unwrap();

    register_mocked_identity_key(
        &mock_keys_server,
        identity_public_key.clone(),
        sign_cacao(
            "com.example.wallet".to_owned(),
            did_pkh.clone(),
            STATEMENT_ALL_DOMAINS.to_owned(),
            identity_public_key.clone(),
            mock_keys_server_url.to_string(),
            account_signing_key,
        ),
    )
    .await;

    let project_id1 = ProjectId::generate();
    let app_domain1 = format!("{project_id1}.example.com");
    subscribe_topic(&project_id1, app_domain1.clone(), &notify_server.url).await;

    let project_id2 = ProjectId::generate();
    let app_domain2 = format!("{project_id2}.example.com");
    subscribe_topic(&project_id2, app_domain2.clone(), &notify_server.url).await;

    let vars = get_vars();
    let (relay_ws_client, mut rx) = create_client(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    let (_subs, _watch_topic_key) = watch_subscriptions(
        notify_server.url.clone(),
        mock_keys_server_url.clone(),
        None,
        &identity_signing_key,
        &identity_public_key.to_did_key(),
        &account.to_did_pkh(),
        &relay_ws_client,
        &mut rx,
    )
    .await;

    // TODO subscribe to project 1
    // TODO assert response
    // TODO assert project 1 subscription in watch subscriptions changed

    // TODO subscribe to project 2
    // TODO assert response
    // TODO assert project 2 subscription in watch subscriptions changed
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn works_with_staging_keys_server(notify_server: &NotifyServerContext) {
    let (identity_signing_key, identity_public_key) = generate_identity_key();

    let (account_signing_key, account) = generate_account();

    let project_id = ProjectId::generate();
    let app_domain = format!("{project_id}.example.com");
    subscribe_topic(&project_id, app_domain.clone(), &notify_server.url).await;

    let keys_server_url = "https://staging.keys.walletconnect.com"
        .parse::<Url>()
        .unwrap();

    assert_successful_response(
        reqwest::Client::builder()
            .build()
            .unwrap()
            .post(keys_server_url.join("/identity").unwrap())
            .json(&CacaoValue {
                cacao: sign_cacao(
                    app_domain.clone(),
                    account.to_did_pkh(),
                    STATEMENT_THIS_DOMAIN.to_owned(),
                    identity_public_key.clone(),
                    keys_server_url.to_string(),
                    account_signing_key,
                ),
            })
            .send()
            .await
            .unwrap(),
    )
    .await;

    let vars = get_vars();
    let (relay_ws_client, mut rx) = create_client(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    let (_subs, _watch_topic_key) = watch_subscriptions(
        notify_server.url.clone(),
        keys_server_url.clone(),
        Some(&app_domain),
        &identity_signing_key,
        &identity_public_key.to_did_key(),
        &account.to_did_pkh(),
        &relay_ws_client,
        &mut rx,
    )
    .await;

    unregister_identity_key(
        keys_server_url,
        account.to_did_pkh(),
        &identity_signing_key,
        identity_public_key.to_did_key(),
    )
    .await;
}

async fn unregister_identity_key(
    keys_server_url: Url,
    did_pkh: String,
    identity_signing_key: &SigningKey,
    identity_did_key: String,
) {
    let unregister_auth = UnregisterIdentityRequestAuth {
        shared_claims: SharedClaims {
            iat: Utc::now().timestamp() as u64,
            exp: Utc::now().timestamp() as u64 + 3600,
            iss: identity_did_key,
            aud: keys_server_url.to_string(),
            act: "unregister_identity".to_owned(),
            mjv: "0".to_owned(),
        },
        pkh: did_pkh,
    };
    let unregister_auth = encode_auth(&unregister_auth, identity_signing_key);
    reqwest::Client::new()
        .delete(keys_server_url.join("/identity").unwrap())
        .body(serde_json::to_string(&json!({"idAuth": unregister_auth})).unwrap())
        .send()
        .await
        .unwrap();
}

// TODO test updating from 1, to 0, to 2 scopes
// TODO test deleting and re-subscribing
// TODO adapt test THIS domain
// TODO assert failure (no response, and no subscriptions changed) when subscribing to project that SIWE doesn't allow
