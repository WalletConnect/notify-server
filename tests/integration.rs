use {
    crate::utils::{
        encode_auth, topic_subscribe, verify_jwt, UnregisterIdentityRequestAuth, JWT_LEEWAY,
    },
    async_trait::async_trait,
    base64::{engine::general_purpose::STANDARD as BASE64, Engine},
    chacha20poly1305::{aead::Aead, ChaCha20Poly1305, KeyInit},
    chrono::{DateTime, Duration, TimeZone, Utc},
    data_encoding::BASE64URL,
    ed25519_dalek::{SigningKey, VerifyingKey},
    hyper::StatusCode,
    notify_server::{
        auth::{
            add_ttl, encode_authentication_private_key, encode_authentication_public_key,
            encode_subscribe_private_key, encode_subscribe_public_key, from_jwt, CacaoValue,
            DidWeb, GetSharedClaims, KeyServerResponse, MessageResponseAuth,
            NotifyServerSubscription, SharedClaims, SubscriptionDeleteRequestAuth,
            SubscriptionDeleteResponseAuth, SubscriptionGetNotificationsRequestAuth,
            SubscriptionGetNotificationsResponseAuth, SubscriptionRequestAuth,
            SubscriptionResponseAuth, SubscriptionUpdateRequestAuth,
            SubscriptionUpdateResponseAuth, WatchSubscriptionsChangedRequestAuth,
            WatchSubscriptionsChangedResponseAuth, WatchSubscriptionsRequestAuth,
            WatchSubscriptionsResponseAuth, KEYS_SERVER_IDENTITY_ENDPOINT,
            KEYS_SERVER_IDENTITY_ENDPOINT_PUBLIC_KEY_QUERY, KEYS_SERVER_STATUS_SUCCESS,
            STATEMENT_ALL_DOMAINS, STATEMENT_THIS_DOMAIN,
        },
        config::Configuration,
        jsonrpc::NotifyPayload,
        model::{
            helpers::{
                get_notifications_for_subscriber, get_project_by_app_domain,
                get_project_by_project_id, get_project_by_topic, get_project_topics,
                get_subscriber_accounts_and_scopes_by_project_id,
                get_subscriber_accounts_by_project_id, get_subscriber_by_topic,
                get_subscriber_topics, get_subscribers_for_project_in,
                get_subscriptions_by_account, upsert_project, upsert_subscriber,
                GetNotificationsParams, GetNotificationsResult, SubscriberAccountAndScopes,
            },
            types::AccountId,
        },
        notify_message::NotifyMessage,
        rate_limit::{self, ClockImpl},
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
            publisher_service::{
                helpers::{
                    dead_letter_give_up_check, dead_letters_check,
                    pick_subscriber_notification_for_processing, upsert_notification,
                    upsert_subscriber_notifications, NotificationToProcess,
                },
                types::SubscriberNotificationStatus,
            },
            websocket_server::{
                decode_key, derive_key, relay_ws_client::RelayClientEvent, AuthMessage,
                NotifyDelete, NotifyRequest, NotifyResponse, NotifySubscribe,
                NotifySubscriptionsChanged, NotifyUpdate, NotifyWatchSubscriptions, ResponseAuth,
            },
        },
        spec::{
            NOTIFY_DELETE_ACT, NOTIFY_DELETE_METHOD, NOTIFY_DELETE_RESPONSE_ACT,
            NOTIFY_DELETE_RESPONSE_TAG, NOTIFY_DELETE_TAG, NOTIFY_DELETE_TTL,
            NOTIFY_GET_NOTIFICATIONS_ACT, NOTIFY_GET_NOTIFICATIONS_RESPONSE_ACT,
            NOTIFY_GET_NOTIFICATIONS_RESPONSE_TAG, NOTIFY_GET_NOTIFICATIONS_TAG,
            NOTIFY_GET_NOTIFICATIONS_TTL, NOTIFY_MESSAGE_ACT, NOTIFY_MESSAGE_METHOD,
            NOTIFY_MESSAGE_RESPONSE_ACT, NOTIFY_MESSAGE_RESPONSE_TAG, NOTIFY_MESSAGE_RESPONSE_TTL,
            NOTIFY_MESSAGE_TAG, NOTIFY_NOOP_TAG, NOTIFY_SUBSCRIBE_ACT, NOTIFY_SUBSCRIBE_METHOD,
            NOTIFY_SUBSCRIBE_RESPONSE_ACT, NOTIFY_SUBSCRIBE_RESPONSE_TAG, NOTIFY_SUBSCRIBE_TAG,
            NOTIFY_SUBSCRIBE_TTL, NOTIFY_SUBSCRIPTIONS_CHANGED_ACT,
            NOTIFY_SUBSCRIPTIONS_CHANGED_METHOD, NOTIFY_SUBSCRIPTIONS_CHANGED_RESPONE_ACT,
            NOTIFY_SUBSCRIPTIONS_CHANGED_RESPONSE_TAG, NOTIFY_SUBSCRIPTIONS_CHANGED_RESPONSE_TTL,
            NOTIFY_SUBSCRIPTIONS_CHANGED_TAG, NOTIFY_UPDATE_ACT, NOTIFY_UPDATE_METHOD,
            NOTIFY_UPDATE_RESPONSE_ACT, NOTIFY_UPDATE_RESPONSE_TAG, NOTIFY_UPDATE_TAG,
            NOTIFY_UPDATE_TTL, NOTIFY_WATCH_SUBSCRIPTIONS_ACT, NOTIFY_WATCH_SUBSCRIPTIONS_METHOD,
            NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_ACT, NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_TAG,
            NOTIFY_WATCH_SUBSCRIPTIONS_TAG, NOTIFY_WATCH_SUBSCRIPTIONS_TTL,
        },
        types::{encode_scope, Envelope, EnvelopeType0, EnvelopeType1, Notification},
        utils::topic_from_key,
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
    serde::de::DeserializeOwned,
    serde_json::{json, Value},
    sha2::{digest::generic_array::GenericArray, Digest},
    sha3::Keccak256,
    sqlx::{postgres::PgPoolOptions, PgPool, Postgres},
    std::{
        collections::HashSet,
        env,
        net::{IpAddr, Ipv4Addr, SocketAddr},
        ops::AddAssign,
        sync::{Arc, RwLock},
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

fn generate_app_domain() -> Arc<str> {
    format!(
        "{}.example.com",
        hex::encode(rand::Rng::gen::<[u8; 10]>(&mut rand::thread_rng()))
    )
    .into()
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
    assert_eq!(project.app_domain, app_domain.as_ref());
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
    assert_eq!(project.app_domain, app_domain.as_ref());
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
    assert_eq!(project.app_domain, app_domain.as_ref());
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
    let subscriber_topic = topic_from_key(&subscriber_sym_key);
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
    let subscriber_topic = topic_from_key(&subscriber_sym_key);
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
    let subscriber_topic2 = topic_from_key(&subscriber_sym_key2);
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
    let subscriber_topic = topic_from_key(&subscriber_sym_key);
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
    let subscriber_topic2 = topic_from_key(&subscriber_sym_key2);
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
        if subscriber.app_domain == app_domain.as_ref() {
            assert_eq!(subscriber.app_domain, app_domain.as_ref());
            assert_eq!(subscriber.account, account_id);
            assert_eq!(subscriber.sym_key, hex::encode(subscriber_sym_key));
            assert_eq!(subscriber.scope, subscriber_scope);
            assert!(subscriber.expiry > Utc::now() + Duration::days(29));
        } else {
            assert_eq!(subscriber.app_domain, app_domain2.as_ref());
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
    let notify_topic = topic_from_key(&notify_key);
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
    let notify_topic = topic_from_key(&notify_key);
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
    let notify_topic = topic_from_key(&notify_key);
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

#[derive(Debug)]
struct MockClock {
    now: RwLock<DateTime<Utc>>,
}

impl MockClock {
    pub fn new(initial_time: DateTime<Utc>) -> Self {
        Self {
            now: RwLock::new(initial_time),
        }
    }

    pub fn advance(&self, duration: Duration) {
        self.now.write().unwrap().add_assign(duration);
    }
}

impl ClockImpl for MockClock {
    fn now(&self) -> DateTime<Utc> {
        *self.now.read().unwrap()
    }
}

struct NotifyServerContext {
    shutdown: broadcast::Sender<()>,
    socket_addr: SocketAddr,
    url: Url,
    postgres: PgPool,
    redis: Arc<Redis>,
    #[allow(dead_code)] // must hold onto MockServer reference or it will shut down
    registry_mock_server: MockServer,
    clock: Arc<MockClock>,
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
        let clock = Arc::new(MockClock::new(Utc::now()));
        // TODO reuse the local configuration defaults here
        let config = Configuration {
            postgres_url,
            postgres_max_connections: 10,
            log_level: "WARN,notify_server=DEBUG".to_string(),
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
            clock: Some(clock.clone()),
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
            clock,
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
    let notify_topic = topic_from_key(&notify_key);
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
    let notify_topic = topic_from_key(&notify_key);
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
    let app_domain = DidWeb::from_domain_arc(generate_app_domain());
    let topic = Topic::generate();
    let subscribe_key = generate_subscribe_key();
    let authentication_key = generate_authentication_key();
    upsert_project(
        project_id.clone(),
        app_domain.domain(),
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
    let notify_topic = topic_from_key(&notify_key);
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

    topic_subscribe(relay_ws_client.as_ref(), notify_topic)
        .await
        .unwrap();

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

    assert_successful_response(
        reqwest::Client::new()
            .post(
                notify_server
                    .url
                    .join(&format!("{project_id}/notify"))
                    .unwrap(),
            )
            .bearer_auth(Uuid::new_v4())
            .json(&notify_body)
            .send()
            .await
            .unwrap(),
    )
    .await;

    let (_, claims) = accept_notify_message(
        &account,
        &authentication_key.verifying_key(),
        &get_client_id(authentication_key.verifying_key()),
        &app_domain,
        &notify_key,
        &mut rx,
    )
    .await;

    assert_eq!(claims.msg.r#type, notification_type);
    assert_eq!(claims.msg.title, "title");
    assert_eq!(claims.msg.body, "body");
    assert_eq!(claims.msg.icon, "");
    assert_eq!(claims.msg.url, "");
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn test_notify_v1(notify_server: &NotifyServerContext) {
    let project_id = ProjectId::generate();
    let app_domain = DidWeb::from_domain_arc(generate_app_domain());
    let topic = Topic::generate();
    let subscribe_key = generate_subscribe_key();
    let authentication_key = generate_authentication_key();
    upsert_project(
        project_id.clone(),
        app_domain.domain(),
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
    let notify_topic = topic_from_key(&notify_key);
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

    topic_subscribe(relay_ws_client.as_ref(), notify_topic)
        .await
        .unwrap();

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

    let response = assert_successful_response(
        reqwest::Client::new()
            .post(
                notify_server
                    .url
                    .join(&format!("/v1/{project_id}/notify"))
                    .unwrap(),
            )
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

    let (_, claims) = accept_notify_message(
        &account,
        &authentication_key.verifying_key(),
        &get_client_id(authentication_key.verifying_key()),
        &app_domain,
        &notify_key,
        &mut rx,
    )
    .await;

    assert_eq!(claims.msg.r#type, notification.r#type);
    assert_eq!(claims.msg.title, notification.title);
    assert_eq!(claims.msg.body, notification.body);
    assert_eq!(claims.msg.icon, notification.icon.unwrap());
    assert_eq!(claims.msg.url, notification.url.unwrap());
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

    let response = assert_successful_response(
        reqwest::Client::new()
            .post(
                notify_server
                    .url
                    .join(&format!("/v1/{project_id}/notify"))
                    .unwrap(),
            )
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
    let notify_topic = topic_from_key(&notify_key);
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

    let response = assert_successful_response(
        reqwest::Client::new()
            .post(
                notify_server
                    .url
                    .join(&format!("/v1/{project_id}/notify"))
                    .unwrap(),
            )
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
    let notify_topic = topic_from_key(&notify_key);
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
    // Note: max_tokens, refill_interval, and refill_rate must be set properly to avoid flaky tests.
    // Although a custom clock is used, the lua script still has expiration logic based on the clock of Redis not this custom one.
    // The formula for the expiration is: math.ceil(((max_tokens - remaining) / refillRate)) * interval
    // If the result of this expression is less than the time between the Redis calls, then the key can expire. Setting refill_duration to 10 seconds and refill_rate to 1 should be enough to avoid this.
    let max_tokens = 2;
    let refill_interval = chrono::Duration::seconds(60);
    let refill_rate = 1;
    let rate_limit = || async {
        rate_limit::token_bucket_many(
            &notify_server.redis,
            vec![key.to_string()],
            max_tokens,
            refill_interval,
            refill_rate,
            &Some(notify_server.clock.clone()),
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
                .signed_duration_since(notify_server.clock.now());
            println!("refill_in: {refill_in}");
            assert!(refill_in > chrono::Duration::zero());
            assert!(refill_in < refill_interval);

            notify_server.clock.advance(refill_interval);

            let result = rate_limit().await;
            assert_eq!(result.0, 0);
        }
    };

    burst().await;

    // Let burst ability recover
    notify_server
        .clock
        .advance(refill_interval * (max_tokens / refill_rate) as i32);

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
            &Some(notify_server.clock.clone()),
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

    let notify = || async {
        reqwest::Client::new()
            .post(
                notify_server
                    .url
                    .join(&format!("/v1/{project_id}/notify"))
                    .unwrap(),
            )
            .bearer_auth(Uuid::new_v4())
            .json(&notify_body)
            .send()
            .await
            .unwrap()
    };

    // Use up the rate limit
    while let Ok(()) = notify_rate_limit(
        &notify_server.redis,
        &project_id,
        &Some(notify_server.clock.clone()),
    )
    .await
    {}

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
    let notify_topic = topic_from_key(&notify_key);
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

    let notify = || async {
        reqwest::Client::new()
            .post(
                notify_server
                    .url
                    .join(&format!("/v1/{project_id}/notify"))
                    .unwrap(),
            )
            .bearer_auth(Uuid::new_v4())
            .json(&notify_body)
            .send()
            .await
            .unwrap()
    };

    for _ in 0..49 {
        let result = subscriber_rate_limit(
            &notify_server.redis,
            &project_id,
            [subscriber_id],
            &Some(notify_server.clock.clone()),
        )
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
    let notify_topic = topic_from_key(&notify_key);
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
    let notify_topic = topic_from_key(&notify_key);
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

    let notify = || async {
        reqwest::Client::new()
            .post(
                notify_server
                    .url
                    .join(&format!("/v1/{project_id}/notify"))
                    .unwrap(),
            )
            .bearer_auth(Uuid::new_v4())
            .json(&notify_body)
            .send()
            .await
            .unwrap()
    };

    for _ in 0..49 {
        let result = subscriber_rate_limit(
            &notify_server.redis,
            &project_id,
            [subscriber_id1],
            &Some(notify_server.clock.clone()),
        )
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
    let notify_topic = topic_from_key(&notify_key);
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

    let response = reqwest::Client::new()
        .post(
            notify_server
                .url
                .join(&format!("/v1/{project_id}/notify"))
                .unwrap(),
        )
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

    let response = reqwest::Client::new()
        .post(
            notify_server
                .url
                .join(&format!("/v1/{project_id}/notify"))
                .unwrap(),
        )
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

    let response = reqwest::Client::new()
        .post(
            notify_server
                .url
                .join(&format!("/v1/{project_id}/notify"))
                .unwrap(),
        )
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
    let subscriber_topic = topic_from_key(&subscriber_sym_key);
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
    let app_domain = generate_app_domain();

    let subscribe_topic_response = reqwest::Client::new()
        .post(
            notify_server
                .url
                .join(&format!("/{project_id}/subscribe-topic",))
                .unwrap(),
        )
        .bearer_auth(Uuid::new_v4())
        .json(&SubscribeTopicRequestBody {
            app_domain: app_domain.clone(),
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
            app_domain: app_domain.clone(),
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

#[derive(Clone)]
struct IdentityKeyDetails {
    keys_server_url: Url,
    signing_key: SigningKey,
    client_id: DecodedClientId,
}

struct TopicEncryptionSchemeAsymetric {
    client_private: x25519_dalek::StaticSecret,
    client_public: x25519_dalek::PublicKey,
    server_public: x25519_dalek::PublicKey,
}

enum TopicEncrptionScheme {
    Asymetric(TopicEncryptionSchemeAsymetric),
    Symetric([u8; 32]),
}

async fn publish_watch_subscriptions_request(
    relay_ws_client: &Client,
    account: &AccountId,
    client_id: &DecodedClientId,
    identity_key_details: &IdentityKeyDetails,
    encryption_details: TopicEncryptionSchemeAsymetric,
    app: Option<DidWeb>,
) {
    publish_jwt_message(
        relay_ws_client,
        client_id,
        identity_key_details,
        &TopicEncrptionScheme::Asymetric(encryption_details),
        NOTIFY_WATCH_SUBSCRIPTIONS_TAG,
        NOTIFY_WATCH_SUBSCRIPTIONS_TTL,
        NOTIFY_WATCH_SUBSCRIPTIONS_ACT,
        |shared_claims| {
            serde_json::to_value(NotifyRequest::new(
                NOTIFY_WATCH_SUBSCRIPTIONS_METHOD,
                NotifyWatchSubscriptions {
                    watch_subscriptions_auth: encode_auth(
                        &WatchSubscriptionsRequestAuth {
                            shared_claims,
                            ksu: identity_key_details.keys_server_url.to_string(),
                            sub: account.to_did_pkh(),
                            app,
                        },
                        &identity_key_details.signing_key,
                    ),
                },
            ))
            .unwrap()
        },
    )
    .await
}

async fn publish_subscribe_request(
    relay_ws_client: &Client,
    did_pkh: String,
    client_id: &DecodedClientId,
    identity_key_details: &IdentityKeyDetails,
    encryption_details: TopicEncryptionSchemeAsymetric,
    app: DidWeb,
    notification_types: HashSet<Uuid>,
) {
    publish_jwt_message(
        relay_ws_client,
        client_id,
        identity_key_details,
        &TopicEncrptionScheme::Asymetric(encryption_details),
        NOTIFY_SUBSCRIBE_TAG,
        NOTIFY_SUBSCRIBE_TTL,
        NOTIFY_SUBSCRIBE_ACT,
        |shared_claims| {
            serde_json::to_value(NotifyRequest::new(
                NOTIFY_SUBSCRIBE_METHOD,
                NotifySubscribe {
                    subscription_auth: encode_auth(
                        &SubscriptionRequestAuth {
                            shared_claims,
                            ksu: identity_key_details.keys_server_url.to_string(),
                            sub: did_pkh.clone(),
                            scp: notification_types
                                .iter()
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                                .join(" "),
                            app,
                        },
                        &identity_key_details.signing_key,
                    ),
                },
            ))
            .unwrap()
        },
    )
    .await
}

async fn accept_message(rx: &mut UnboundedReceiver<RelayClientEvent>) -> PublishedMessage {
    let event = rx.recv().await.unwrap();
    match event {
        RelayClientEvent::Message(msg) => msg,
        e => panic!("Expected message, got {e:?}"),
    }
}

#[allow(clippy::too_many_arguments)]
async fn subscribe(
    relay_ws_client: &relay_client::websocket::Client,
    rx: &mut UnboundedReceiver<RelayClientEvent>,
    account: &AccountId,
    identity_key_details: &IdentityKeyDetails,
    app_key_agreement_key: x25519_dalek::PublicKey,
    app_client_id: &DecodedClientId,
    app: DidWeb,
    notification_types: HashSet<Uuid>,
) {
    let secret = StaticSecret::random_from_rng(OsRng);
    let public = PublicKey::from(&secret);
    let response_topic_key = derive_key(&app_key_agreement_key, &secret).unwrap();
    let response_topic = topic_from_key(&response_topic_key);

    publish_subscribe_request(
        relay_ws_client,
        account.to_did_pkh(),
        app_client_id,
        identity_key_details,
        TopicEncryptionSchemeAsymetric {
            client_private: secret,
            client_public: public,
            server_public: app_key_agreement_key,
        },
        app,
        notification_types,
    )
    .await;

    topic_subscribe(relay_ws_client, response_topic.clone())
        .await
        .unwrap();

    let msg = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let msg = accept_message(rx).await;
            if msg.tag == NOTIFY_SUBSCRIBE_RESPONSE_TAG && msg.topic == response_topic {
                return msg;
            }
        }
    })
    .await
    .unwrap();

    let (_id, auth) = decode_response_message::<SubscriptionResponseAuth>(msg, &response_topic_key);
    assert_eq!(auth.shared_claims.act, NOTIFY_SUBSCRIBE_RESPONSE_ACT);
    assert_eq!(auth.shared_claims.iss, app_client_id.to_did_key());
    assert_eq!(
        auth.shared_claims.aud,
        identity_key_details.client_id.to_did_key()
    );
}

#[allow(clippy::too_many_arguments)]
async fn publish_jwt_message(
    relay_ws_client: &Client,
    client_id: &DecodedClientId,
    identity_key_details: &IdentityKeyDetails,
    encryption_details: &TopicEncrptionScheme,
    tag: u32,
    ttl: std::time::Duration,
    act: &str,
    make_message: impl FnOnce(SharedClaims) -> serde_json::Value,
) {
    fn make_shared_claims(
        now: DateTime<Utc>,
        ttl: std::time::Duration,
        act: &str,
        client_id: &DecodedClientId,
        identity_key_details: &IdentityKeyDetails,
    ) -> SharedClaims {
        SharedClaims {
            iat: now.timestamp() as u64,
            exp: add_ttl(now, ttl).timestamp() as u64,
            iss: identity_key_details.client_id.to_did_key(),
            act: act.to_owned(),
            aud: client_id.to_did_key(),
            mjv: "0".to_owned(),
        }
    }

    let now = Utc::now();

    let message = make_message(make_shared_claims(
        now,
        ttl,
        act,
        client_id,
        identity_key_details,
    ));

    let (envelope, topic) = match encryption_details {
        TopicEncrptionScheme::Asymetric(TopicEncryptionSchemeAsymetric {
            client_private: client_secret,
            client_public,
            server_public,
        }) => {
            let response_topic_key = derive_key(server_public, client_secret).unwrap();
            (
                Envelope::<EnvelopeType1>::new(
                    &response_topic_key,
                    message,
                    *client_public.as_bytes(),
                )
                .unwrap()
                .to_bytes(),
                topic_from_key(server_public.as_bytes()),
            )
        }
        TopicEncrptionScheme::Symetric(sym_key) => (
            Envelope::<EnvelopeType0>::new(sym_key, message)
                .unwrap()
                .to_bytes(),
            topic_from_key(sym_key),
        ),
    };

    let message = BASE64.encode(envelope);

    relay_ws_client
        .publish(topic, message, tag, ttl, false)
        .await
        .unwrap();
}

fn decode_message<T>(msg: PublishedMessage, key: &[u8; 32]) -> T
where
    T: DeserializeOwned,
{
    let Envelope::<EnvelopeType0> { sealbox, iv, .. } =
        Envelope::<EnvelopeType0>::from_bytes(BASE64.decode(msg.message.as_bytes()).unwrap())
            .unwrap();
    let decrypted_response = ChaCha20Poly1305::new(GenericArray::from_slice(key))
        .decrypt(&iv.into(), chacha20poly1305::aead::Payload::from(&*sealbox))
        .unwrap();
    serde_json::from_slice::<T>(&decrypted_response).unwrap()
}

fn decode_response_message<T>(msg: PublishedMessage, key: &[u8; 32]) -> (u64, T)
where
    T: GetSharedClaims + DeserializeOwned,
{
    let response = decode_message::<NotifyResponse<ResponseAuth>>(msg, key);
    (
        response.id,
        from_jwt::<T>(&response.result.response_auth).unwrap(),
    )
}

fn decode_auth_message<T>(msg: PublishedMessage, key: &[u8; 32]) -> (u64, T)
where
    T: GetSharedClaims + DeserializeOwned,
{
    let response = decode_message::<NotifyResponse<AuthMessage>>(msg, key);
    (response.id, from_jwt::<T>(&response.result.auth).unwrap())
}

#[allow(clippy::too_many_arguments)]
async fn watch_subscriptions(
    notify_server_url: Url,
    identity_key_details: &IdentityKeyDetails,
    app_domain: Option<DidWeb>,
    account: &AccountId,
    relay_ws_client: &relay_client::websocket::Client,
    rx: &mut UnboundedReceiver<RelayClientEvent>,
) -> (Vec<NotifyServerSubscription>, [u8; 32], DecodedClientId) {
    let (key_agreement_key, client_id) = get_notify_did_json(&notify_server_url).await;

    let secret = StaticSecret::random_from_rng(OsRng);
    let public = PublicKey::from(&secret);

    let response_topic_key = derive_key(&key_agreement_key, &secret).unwrap();
    let response_topic = topic_from_key(&response_topic_key);

    publish_watch_subscriptions_request(
        relay_ws_client,
        account,
        &client_id,
        identity_key_details,
        TopicEncryptionSchemeAsymetric {
            client_private: secret,
            client_public: public,
            server_public: key_agreement_key,
        },
        app_domain,
    )
    .await;

    topic_subscribe(relay_ws_client, response_topic.clone())
        .await
        .unwrap();

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

    let (_id, auth) =
        decode_response_message::<WatchSubscriptionsResponseAuth>(msg, &response_topic_key);
    assert_eq!(
        auth.shared_claims.act,
        NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_ACT
    );
    assert_eq!(auth.shared_claims.iss, client_id.to_did_key());
    assert_eq!(
        auth.shared_claims.aud,
        identity_key_details.client_id.to_did_key()
    );

    (auth.sbs, response_topic_key, client_id)
}

async fn publish_subscriptions_changed_response(
    relay_ws_client: &Client,
    did_pkh: &AccountId,
    client_id: &DecodedClientId,
    identity_key_details: &IdentityKeyDetails,
    sym_key: [u8; 32],
    id: u64,
) {
    publish_jwt_message(
        relay_ws_client,
        client_id,
        identity_key_details,
        &TopicEncrptionScheme::Symetric(sym_key),
        NOTIFY_SUBSCRIPTIONS_CHANGED_RESPONSE_TAG,
        NOTIFY_SUBSCRIPTIONS_CHANGED_RESPONSE_TTL,
        NOTIFY_SUBSCRIPTIONS_CHANGED_RESPONE_ACT,
        |shared_claims| {
            serde_json::to_value(NotifyResponse::new(
                id,
                ResponseAuth {
                    response_auth: encode_auth(
                        &WatchSubscriptionsChangedResponseAuth {
                            shared_claims,
                            ksu: identity_key_details.keys_server_url.to_string(),
                            sub: did_pkh.to_did_pkh(),
                        },
                        &identity_key_details.signing_key,
                    ),
                },
            ))
            .unwrap()
        },
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn accept_watch_subscriptions_changed(
    notify_server_client_id: &DecodedClientId,
    identity_key_details: &IdentityKeyDetails,
    account: &AccountId,
    watch_topic_key: [u8; 32],
    relay_ws_client: &relay_client::websocket::Client,
    rx: &mut UnboundedReceiver<RelayClientEvent>,
) -> Vec<NotifyServerSubscription> {
    let msg = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let msg = accept_message(rx).await;
            if msg.tag == NOTIFY_SUBSCRIPTIONS_CHANGED_TAG
                && msg.topic == topic_from_key(&watch_topic_key)
            {
                return msg;
            }
        }
    })
    .await
    .unwrap();

    let request =
        decode_message::<NotifyRequest<NotifySubscriptionsChanged>>(msg, &watch_topic_key);
    assert_eq!(request.method, NOTIFY_SUBSCRIPTIONS_CHANGED_METHOD);
    let auth = from_jwt::<WatchSubscriptionsChangedRequestAuth>(
        &request.params.subscriptions_changed_auth,
    )
    .unwrap();

    assert_eq!(auth.shared_claims.act, NOTIFY_SUBSCRIPTIONS_CHANGED_ACT);
    assert_eq!(auth.shared_claims.iss, notify_server_client_id.to_did_key());
    assert_eq!(
        auth.shared_claims.aud,
        identity_key_details.client_id.to_did_key()
    );

    publish_subscriptions_changed_response(
        relay_ws_client,
        account,
        notify_server_client_id,
        identity_key_details,
        watch_topic_key,
        request.id,
    )
    .await;

    auth.sbs
}

async fn publish_notify_message_response(
    relay_ws_client: &Client,
    account: &AccountId,
    app_client_id: &DecodedClientId,
    did_web: DidWeb,
    identity_key_details: &IdentityKeyDetails,
    sym_key: [u8; 32],
    id: u64,
) {
    publish_jwt_message(
        relay_ws_client,
        app_client_id,
        identity_key_details,
        &TopicEncrptionScheme::Symetric(sym_key),
        NOTIFY_MESSAGE_RESPONSE_TAG,
        NOTIFY_MESSAGE_RESPONSE_TTL,
        NOTIFY_MESSAGE_RESPONSE_ACT,
        |shared_claims| {
            serde_json::to_value(NotifyResponse::new(
                id,
                ResponseAuth {
                    response_auth: encode_auth(
                        &MessageResponseAuth {
                            shared_claims,
                            ksu: identity_key_details.keys_server_url.to_string(),
                            sub: account.to_did_pkh(),
                            app: did_web,
                        },
                        &identity_key_details.signing_key,
                    ),
                },
            ))
            .unwrap()
        },
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn accept_notify_message(
    account: &AccountId,
    app_authentication: &VerifyingKey,
    app_client_id: &DecodedClientId,
    app_domain: &DidWeb,
    notify_key: &[u8; 32],
    rx: &mut UnboundedReceiver<RelayClientEvent>,
) -> (u64, NotifyMessage) {
    let msg = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let msg = accept_message(rx).await;
            if msg.tag == NOTIFY_MESSAGE_TAG && msg.topic == topic_from_key(notify_key) {
                return msg;
            }
        }
    })
    .await
    .unwrap();

    let request = decode_message::<NotifyRequest<NotifyPayload>>(msg, notify_key);
    assert_eq!(request.method, NOTIFY_MESSAGE_METHOD);

    let claims = verify_jwt(&request.params.message_auth, app_authentication).unwrap();

    assert_eq!(claims.iss, app_client_id.to_did_key());
    assert_eq!(claims.sub, account.to_did_pkh());
    assert!(claims.iat < chrono::Utc::now().timestamp() + JWT_LEEWAY); // TODO remove leeway
    assert!(claims.exp > chrono::Utc::now().timestamp() - JWT_LEEWAY); // TODO remove leeway
    assert_eq!(claims.app.as_ref(), app_domain.domain()); // bug: https://github.com/WalletConnect/notify-server/issues/251
    assert_eq!(claims.act, NOTIFY_MESSAGE_ACT);

    (request.id, claims)
}

#[allow(clippy::too_many_arguments)]
async fn accept_and_respond_to_notify_message(
    identity_key_details: &IdentityKeyDetails,
    account: &AccountId,
    app_authentication: &VerifyingKey,
    app_client_id: &DecodedClientId,
    app_domain: DidWeb,
    notify_key: [u8; 32],
    relay_ws_client: &relay_client::websocket::Client,
    rx: &mut UnboundedReceiver<RelayClientEvent>,
) -> NotifyMessage {
    let (request_id, claims) = accept_notify_message(
        account,
        app_authentication,
        app_client_id,
        &app_domain,
        &notify_key,
        rx,
    )
    .await;

    publish_notify_message_response(
        relay_ws_client,
        account,
        app_client_id,
        app_domain,
        identity_key_details,
        notify_key,
        request_id,
    )
    .await;

    claims
}

async fn publish_update_request(
    relay_ws_client: &Client,
    account: &AccountId,
    client_id: &DecodedClientId,
    identity_key_details: &IdentityKeyDetails,
    sym_key: [u8; 32],
    app: DidWeb,
    notification_types: &HashSet<Uuid>,
) {
    publish_jwt_message(
        relay_ws_client,
        client_id,
        identity_key_details,
        &TopicEncrptionScheme::Symetric(sym_key),
        NOTIFY_UPDATE_TAG,
        NOTIFY_UPDATE_TTL,
        NOTIFY_UPDATE_ACT,
        |shared_claims| {
            serde_json::to_value(NotifyRequest::new(
                NOTIFY_UPDATE_METHOD,
                NotifyUpdate {
                    update_auth: encode_auth(
                        &SubscriptionUpdateRequestAuth {
                            shared_claims,
                            ksu: identity_key_details.keys_server_url.to_string(),
                            sub: account.to_did_pkh(),
                            app,
                            scp: encode_scope(notification_types),
                        },
                        &identity_key_details.signing_key,
                    ),
                },
            ))
            .unwrap()
        },
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn update(
    relay_ws_client: &relay_client::websocket::Client,
    rx: &mut UnboundedReceiver<RelayClientEvent>,
    account: &AccountId,
    identity_key_details: &IdentityKeyDetails,
    app: &DidWeb,
    app_client_id: &DecodedClientId,
    notify_key: [u8; 32],
    notification_types: &HashSet<Uuid>,
) {
    publish_update_request(
        relay_ws_client,
        account,
        app_client_id,
        identity_key_details,
        notify_key,
        app.clone(),
        notification_types,
    )
    .await;

    let response_topic = topic_from_key(&notify_key);
    let msg = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let msg = accept_message(rx).await;
            if msg.tag == NOTIFY_UPDATE_RESPONSE_TAG && msg.topic == response_topic {
                return msg;
            }
        }
    })
    .await
    .unwrap();

    let (_id, auth) = decode_response_message::<SubscriptionUpdateResponseAuth>(msg, &notify_key);
    assert_eq!(auth.shared_claims.act, NOTIFY_UPDATE_RESPONSE_ACT);
    assert_eq!(auth.shared_claims.iss, app_client_id.to_did_key());
    assert_eq!(
        auth.shared_claims.aud,
        identity_key_details.client_id.to_did_key()
    );
    assert_eq!(&auth.app, app);
}

async fn publish_delete_request(
    relay_ws_client: &Client,
    account: &AccountId,
    client_id: &DecodedClientId,
    identity_key_details: &IdentityKeyDetails,
    sym_key: [u8; 32],
    app: DidWeb,
) {
    publish_jwt_message(
        relay_ws_client,
        client_id,
        identity_key_details,
        &TopicEncrptionScheme::Symetric(sym_key),
        NOTIFY_DELETE_TAG,
        NOTIFY_DELETE_TTL,
        NOTIFY_DELETE_ACT,
        |shared_claims| {
            serde_json::to_value(NotifyRequest::new(
                NOTIFY_DELETE_METHOD,
                NotifyDelete {
                    delete_auth: encode_auth(
                        &SubscriptionDeleteRequestAuth {
                            shared_claims,
                            ksu: identity_key_details.keys_server_url.to_string(),
                            sub: account.to_did_pkh(),
                            app,
                        },
                        &identity_key_details.signing_key,
                    ),
                },
            ))
            .unwrap()
        },
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn delete(
    identity_key_details: &IdentityKeyDetails,
    app: &DidWeb,
    app_client_id: &DecodedClientId,
    account: &AccountId,
    notify_key: [u8; 32],
    relay_ws_client: &relay_client::websocket::Client,
    rx: &mut UnboundedReceiver<RelayClientEvent>,
) {
    publish_delete_request(
        relay_ws_client,
        account,
        app_client_id,
        identity_key_details,
        notify_key,
        app.clone(),
    )
    .await;

    let response_topic = topic_from_key(&notify_key);
    let msg = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let msg = accept_message(rx).await;
            if msg.tag == NOTIFY_DELETE_RESPONSE_TAG && msg.topic == response_topic {
                return msg;
            }
        }
    })
    .await
    .unwrap();

    let (_id, auth) = decode_response_message::<SubscriptionDeleteResponseAuth>(msg, &notify_key);
    assert_eq!(auth.shared_claims.act, NOTIFY_DELETE_RESPONSE_ACT);
    assert_eq!(auth.shared_claims.iss, app_client_id.to_did_key());
    assert_eq!(
        auth.shared_claims.aud,
        identity_key_details.client_id.to_did_key()
    );
    assert_eq!(&auth.app, app);
}

async fn publish_get_notifications_request(
    relay_ws_client: &Client,
    account: &AccountId,
    client_id: &DecodedClientId,
    identity_key_details: &IdentityKeyDetails,
    sym_key: [u8; 32],
    app: DidWeb,
    params: GetNotificationsParams,
) {
    publish_jwt_message(
        relay_ws_client,
        client_id,
        identity_key_details,
        &TopicEncrptionScheme::Symetric(sym_key),
        NOTIFY_GET_NOTIFICATIONS_TAG,
        NOTIFY_GET_NOTIFICATIONS_TTL,
        NOTIFY_GET_NOTIFICATIONS_ACT,
        |shared_claims| {
            serde_json::to_value(NotifyRequest::new(
                NOTIFY_UPDATE_METHOD,
                AuthMessage {
                    auth: encode_auth(
                        &SubscriptionGetNotificationsRequestAuth {
                            shared_claims,
                            ksu: identity_key_details.keys_server_url.to_string(),
                            sub: account.to_did_pkh(),
                            app,
                            params,
                        },
                        &identity_key_details.signing_key,
                    ),
                },
            ))
            .unwrap()
        },
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn get_notifications(
    relay_ws_client: &relay_client::websocket::Client,
    rx: &mut UnboundedReceiver<RelayClientEvent>,
    account: &AccountId,
    identity_key_details: &IdentityKeyDetails,
    app: &DidWeb,
    app_client_id: &DecodedClientId,
    notify_key: [u8; 32],
    params: GetNotificationsParams,
) -> GetNotificationsResult {
    publish_get_notifications_request(
        relay_ws_client,
        account,
        app_client_id,
        identity_key_details,
        notify_key,
        app.clone(),
        params,
    )
    .await;

    let response_topic = topic_from_key(&notify_key);
    let msg = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let msg = accept_message(rx).await;
            if msg.tag == NOTIFY_GET_NOTIFICATIONS_RESPONSE_TAG && msg.topic == response_topic {
                return msg;
            }
        }
    })
    .await
    .unwrap();

    let (_id, auth) =
        decode_auth_message::<SubscriptionGetNotificationsResponseAuth>(msg, &notify_key);
    assert_eq!(
        auth.shared_claims.act,
        NOTIFY_GET_NOTIFICATIONS_RESPONSE_ACT
    );
    assert_eq!(auth.shared_claims.iss, app_client_id.to_did_key());
    assert_eq!(
        auth.shared_claims.aud,
        identity_key_details.client_id.to_did_key()
    );
    assert_eq!(&auth.app, app);

    let value = serde_json::to_value(&auth).unwrap();
    assert!(value.get("sub").is_some());
    assert!(value.get("nfs").is_some());
    assert!(value.get("mre").is_some());
    assert!(value.get("notifications").is_none());
    assert!(value.get("has_more").is_none());

    auth.result
}

fn generate_identity_key() -> (SigningKey, DecodedClientId) {
    let keypair = Keypair::generate(&mut StdRng::from_entropy());
    let signing_key = SigningKey::from_bytes(keypair.secret_key().as_bytes());
    let client_id = DecodedClientId::from_key(&keypair.public_key());
    (signing_key, client_id)
}

fn sign_cacao(
    app_domain: &DidWeb,
    account: &AccountId,
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
            domain: app_domain.domain().to_owned(),
            iss: account.to_did_pkh(),
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

fn get_client_id(verifying_key: ed25519_dalek::VerifyingKey) -> DecodedClientId {
    // Better approach, but dependency versions conflict right now.
    // See: https://github.com/WalletConnect/WalletConnectRust/issues/53
    // DecodedClientId::from_key(verifying_key)
    DecodedClientId(verifying_key.to_bytes())
}

async fn subscribe_topic(
    project_id: &ProjectId,
    app_domain: DidWeb,
    notify_server_url: &Url,
) -> (
    x25519_dalek::PublicKey,
    ed25519_dalek::VerifyingKey,
    DecodedClientId,
) {
    let response = assert_successful_response(
        reqwest::Client::new()
            .post(
                notify_server_url
                    .join(&format!("/{project_id}/subscribe-topic",))
                    .unwrap(),
            )
            .bearer_auth(Uuid::new_v4())
            .json(&SubscribeTopicRequestBody {
                app_domain: app_domain.into_domain(),
            })
            .send()
            .await
            .unwrap(),
    )
    .await
    .json::<SubscribeTopicResponseBody>()
    .await
    .unwrap();

    let authentication = decode_key(&response.authentication_key).unwrap();
    let key_agreement = decode_key(&response.subscribe_key).unwrap();

    let key_agreement = x25519_dalek::PublicKey::from(key_agreement);
    let authentication = ed25519_dalek::VerifyingKey::from_bytes(&authentication).unwrap();
    let client_id = get_client_id(authentication);
    (key_agreement, authentication, client_id)
}

async fn unregister_identity_key(
    keys_server_url: Url,
    account: &AccountId,
    identity_signing_key: &SigningKey,
    identity_did_key: &DecodedClientId,
) {
    let unregister_auth = UnregisterIdentityRequestAuth {
        shared_claims: SharedClaims {
            iat: Utc::now().timestamp() as u64,
            exp: Utc::now().timestamp() as u64 + 3600,
            iss: identity_did_key.to_did_key(),
            aud: keys_server_url.to_string(),
            act: "unregister_identity".to_owned(),
            mjv: "0".to_owned(),
        },
        pkh: account.to_did_pkh(),
    };
    let unregister_auth = encode_auth(&unregister_auth, identity_signing_key);
    reqwest::Client::new()
        .delete(keys_server_url.join("/identity").unwrap())
        .body(serde_json::to_string(&json!({"idAuth": unregister_auth})).unwrap())
        .send()
        .await
        .unwrap();
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn update_subscription(notify_server: &NotifyServerContext) {
    let (account_signing_key, account) = generate_account();

    let keys_server = MockServer::start().await;
    let keys_server_url = keys_server.uri().parse::<Url>().unwrap();

    let (identity_signing_key, identity_public_key) = generate_identity_key();
    let identity_key_details = IdentityKeyDetails {
        keys_server_url,
        signing_key: identity_signing_key,
        client_id: identity_public_key.clone(),
    };

    let project_id = ProjectId::generate();
    let app_domain = DidWeb::from_domain(format!("{project_id}.walletconnect.com"));

    register_mocked_identity_key(
        &keys_server,
        identity_public_key.clone(),
        sign_cacao(
            &app_domain,
            &account,
            STATEMENT_THIS_DOMAIN.to_owned(),
            identity_public_key.clone(),
            identity_key_details.keys_server_url.to_string(),
            account_signing_key,
        ),
    )
    .await;

    let vars = get_vars();
    let (relay_ws_client, mut rx) = create_client(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    let (key_agreement, _authentication, client_id) =
        subscribe_topic(&project_id, app_domain.clone(), &notify_server.url).await;

    let (subs, watch_topic_key, notify_server_client_id) = watch_subscriptions(
        notify_server.url.clone(),
        &identity_key_details,
        Some(app_domain.clone()),
        &account,
        &relay_ws_client,
        &mut rx,
    )
    .await;
    assert!(subs.is_empty());

    // Subscribe with 1 type
    let notification_types = HashSet::from([Uuid::new_v4()]);
    subscribe(
        &relay_ws_client,
        &mut rx,
        &account,
        &identity_key_details,
        key_agreement,
        &client_id,
        app_domain.clone(),
        notification_types.clone(),
    )
    .await;
    let subs = accept_watch_subscriptions_changed(
        &notify_server_client_id,
        &identity_key_details,
        &account,
        watch_topic_key,
        &relay_ws_client,
        &mut rx,
    )
    .await;
    assert_eq!(subs.len(), 1);
    let sub = &subs[0];
    assert_eq!(sub.scope, notification_types);

    let notify_key = decode_key(&sub.sym_key).unwrap();

    topic_subscribe(relay_ws_client.as_ref(), topic_from_key(&notify_key))
        .await
        .unwrap();

    // Update to 0 types
    let notification_types = HashSet::from([]);
    update(
        &relay_ws_client,
        &mut rx,
        &account,
        &identity_key_details,
        &app_domain,
        &client_id,
        notify_key,
        &notification_types,
    )
    .await;
    let subs = accept_watch_subscriptions_changed(
        &notify_server_client_id,
        &identity_key_details,
        &account,
        watch_topic_key,
        &relay_ws_client,
        &mut rx,
    )
    .await;
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0].scope, notification_types);

    // Update to 2 types
    let notification_types = HashSet::from([Uuid::new_v4(), Uuid::new_v4()]);
    update(
        &relay_ws_client,
        &mut rx,
        &account,
        &identity_key_details,
        &app_domain,
        &client_id,
        notify_key,
        &notification_types,
    )
    .await;
    let subs = accept_watch_subscriptions_changed(
        &notify_server_client_id,
        &identity_key_details,
        &account,
        watch_topic_key,
        &relay_ws_client,
        &mut rx,
    )
    .await;
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0].scope, notification_types);
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn sends_noop(notify_server: &NotifyServerContext) {
    let (account_signing_key, account) = generate_account();

    let keys_server = MockServer::start().await;
    let keys_server_url = keys_server.uri().parse::<Url>().unwrap();

    let (identity_signing_key, identity_public_key) = generate_identity_key();
    let identity_key_details = IdentityKeyDetails {
        keys_server_url,
        signing_key: identity_signing_key,
        client_id: identity_public_key.clone(),
    };

    let project_id = ProjectId::generate();
    let app_domain = DidWeb::from_domain(format!("{project_id}.walletconnect.com"));

    register_mocked_identity_key(
        &keys_server,
        identity_public_key.clone(),
        sign_cacao(
            &app_domain,
            &account,
            STATEMENT_THIS_DOMAIN.to_owned(),
            identity_public_key.clone(),
            identity_key_details.keys_server_url.to_string(),
            account_signing_key,
        ),
    )
    .await;

    let vars = get_vars();
    let (relay_ws_client, mut rx) = create_client(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    let (key_agreement, _authentication, client_id) =
        subscribe_topic(&project_id, app_domain.clone(), &notify_server.url).await;

    let (subs, watch_topic_key, notify_server_client_id) = watch_subscriptions(
        notify_server.url.clone(),
        &identity_key_details,
        Some(app_domain.clone()),
        &account,
        &relay_ws_client,
        &mut rx,
    )
    .await;
    assert!(subs.is_empty());

    let notification_type = Uuid::new_v4();
    let notification_types = HashSet::from([notification_type]);
    subscribe(
        &relay_ws_client,
        &mut rx,
        &account,
        &identity_key_details,
        key_agreement,
        &client_id,
        app_domain.clone(),
        notification_types.clone(),
    )
    .await;

    let subs = accept_watch_subscriptions_changed(
        &notify_server_client_id,
        &identity_key_details,
        &account,
        watch_topic_key,
        &relay_ws_client,
        &mut rx,
    )
    .await;
    assert_eq!(subs.len(), 1);
    let sub = &subs[0];
    assert_eq!(sub.scope, notification_types);

    let notify_key = decode_key(&sub.sym_key).unwrap();
    let notify_topic = topic_from_key(&notify_key);

    topic_subscribe(relay_ws_client.as_ref(), notify_topic.clone())
        .await
        .unwrap();

    let msg = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let msg = accept_message(&mut rx).await;
            if msg.tag == NOTIFY_NOOP_TAG && msg.topic == notify_topic {
                return msg;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(msg.message.as_ref(), "");
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn delete_subscription(notify_server: &NotifyServerContext) {
    let (account_signing_key, account) = generate_account();

    let keys_server = MockServer::start().await;
    let keys_server_url = keys_server.uri().parse::<Url>().unwrap();

    let (identity_signing_key, identity_public_key) = generate_identity_key();
    let identity_key_details = IdentityKeyDetails {
        keys_server_url,
        signing_key: identity_signing_key,
        client_id: identity_public_key.clone(),
    };

    let project_id = ProjectId::generate();
    let app_domain = DidWeb::from_domain(format!("{project_id}.walletconnect.com"));

    register_mocked_identity_key(
        &keys_server,
        identity_public_key.clone(),
        sign_cacao(
            &app_domain,
            &account,
            STATEMENT_THIS_DOMAIN.to_owned(),
            identity_public_key.clone(),
            identity_key_details.keys_server_url.to_string(),
            account_signing_key,
        ),
    )
    .await;

    let vars = get_vars();
    let (relay_ws_client, mut rx) = create_client(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    let (key_agreement, authentication, client_id) =
        subscribe_topic(&project_id, app_domain.clone(), &notify_server.url).await;

    let (subs, watch_topic_key, notify_server_client_id) = watch_subscriptions(
        notify_server.url.clone(),
        &identity_key_details,
        Some(app_domain.clone()),
        &account,
        &relay_ws_client,
        &mut rx,
    )
    .await;
    assert!(subs.is_empty());

    let notification_type = Uuid::new_v4();
    let notification_types = HashSet::from([notification_type]);
    subscribe(
        &relay_ws_client,
        &mut rx,
        &account,
        &identity_key_details,
        key_agreement,
        &client_id,
        app_domain.clone(),
        notification_types.clone(),
    )
    .await;

    let subs = accept_watch_subscriptions_changed(
        &notify_server_client_id,
        &identity_key_details,
        &account,
        watch_topic_key,
        &relay_ws_client,
        &mut rx,
    )
    .await;
    assert_eq!(subs.len(), 1);
    let sub = &subs[0];
    assert_eq!(sub.scope, notification_types);

    let notify_key = decode_key(&sub.sym_key).unwrap();

    topic_subscribe(relay_ws_client.as_ref(), topic_from_key(&notify_key))
        .await
        .unwrap();

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

    assert_successful_response(
        reqwest::Client::new()
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
            .unwrap(),
    )
    .await;

    let claims = accept_and_respond_to_notify_message(
        &identity_key_details,
        &account,
        &authentication,
        &client_id,
        app_domain.clone(),
        notify_key,
        &relay_ws_client,
        &mut rx,
    )
    .await;

    assert_eq!(claims.msg.r#type, notification.r#type);
    assert_eq!(claims.msg.title, notification.title);
    assert_eq!(claims.msg.body, notification.body);
    assert_eq!(claims.msg.icon, "icon");
    assert_eq!(claims.msg.url, "url");

    delete(
        &identity_key_details,
        &app_domain,
        &client_id,
        &account,
        notify_key,
        &relay_ws_client,
        &mut rx,
    )
    .await;

    let sbs = accept_watch_subscriptions_changed(
        &notify_server_client_id,
        &identity_key_details,
        &account,
        watch_topic_key,
        &relay_ws_client,
        &mut rx,
    )
    .await;
    assert!(sbs.is_empty());

    let resp = assert_successful_response(
        reqwest::Client::new()
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
            .unwrap(),
    )
    .await
    .json::<notify_server::services::public_http_server::handlers::notify_v0::Response>()
    .await
    .unwrap();

    assert_eq!(resp.not_found.len(), 1);
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
async fn all_domains_works(notify_server: &NotifyServerContext) {
    let (account_signing_key, account) = generate_account();

    let keys_server = MockServer::start().await;
    let keys_server_url = keys_server.uri().parse::<Url>().unwrap();

    let (identity_signing_key, identity_public_key) = generate_identity_key();
    let identity_key_details = IdentityKeyDetails {
        keys_server_url,
        signing_key: identity_signing_key,
        client_id: identity_public_key.clone(),
    };

    register_mocked_identity_key(
        &keys_server,
        identity_public_key.clone(),
        sign_cacao(
            &DidWeb::from_domain("com.example.wallet".to_owned()),
            &account,
            STATEMENT_ALL_DOMAINS.to_owned(),
            identity_public_key.clone(),
            identity_key_details.keys_server_url.to_string(),
            account_signing_key,
        ),
    )
    .await;

    let project_id1 = ProjectId::generate();
    let app_domain1 = DidWeb::from_domain(format!("{project_id1}.example.com"));
    let (key_agreement1, authentication1, client_id1) =
        subscribe_topic(&project_id1, app_domain1.clone(), &notify_server.url).await;

    let project_id2 = ProjectId::generate();
    let app_domain2 = DidWeb::from_domain(format!("{project_id2}.example.com"));
    let (key_agreement2, authentication2, client_id2) =
        subscribe_topic(&project_id2, app_domain2.clone(), &notify_server.url).await;

    let vars = get_vars();
    let (relay_ws_client, mut rx) = create_client(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    let (subs, watch_topic_key, notify_server_client_id) = watch_subscriptions(
        notify_server.url.clone(),
        &identity_key_details,
        None,
        &account,
        &relay_ws_client,
        &mut rx,
    )
    .await;
    assert!(subs.is_empty());

    let notification_type1 = Uuid::new_v4();
    let notification_types1 = HashSet::from([notification_type1, Uuid::new_v4()]);
    subscribe(
        &relay_ws_client,
        &mut rx,
        &account,
        &identity_key_details,
        key_agreement1,
        &client_id1,
        app_domain1.clone(),
        notification_types1.clone(),
    )
    .await;
    let subs = accept_watch_subscriptions_changed(
        &notify_server_client_id,
        &identity_key_details,
        &account,
        watch_topic_key,
        &relay_ws_client,
        &mut rx,
    )
    .await;
    assert_eq!(subs.len(), 1);
    let sub = &subs[0];
    assert_eq!(sub.scope, notification_types1);
    assert_eq!(sub.account, account);
    assert_eq!(sub.app_domain, app_domain1.domain());
    assert_eq!(sub.app_authentication_key, client_id1.to_did_key());
    assert_eq!(
        &DecodedClientId::try_from_did_key(&sub.app_authentication_key)
            .unwrap()
            .0,
        authentication1.as_bytes()
    );

    let notification_type2 = Uuid::new_v4();
    let notification_types2 = HashSet::from([notification_type2, Uuid::new_v4()]);
    subscribe(
        &relay_ws_client,
        &mut rx,
        &account,
        &identity_key_details,
        key_agreement2,
        &client_id2,
        app_domain2.clone(),
        notification_types2.clone(),
    )
    .await;
    let subs = accept_watch_subscriptions_changed(
        &notify_server_client_id,
        &identity_key_details,
        &account,
        watch_topic_key,
        &relay_ws_client,
        &mut rx,
    )
    .await;
    assert_eq!(subs.len(), 2);
    let sub1 = subs
        .iter()
        .find(|sub| sub.app_domain == app_domain1.domain())
        .unwrap();
    assert_eq!(sub1.scope, notification_types1);
    assert_eq!(sub1.account, account);
    assert_eq!(sub1.app_domain, app_domain1.domain());
    assert_eq!(sub1.app_authentication_key, client_id1.to_did_key());
    assert_eq!(
        &DecodedClientId::try_from_did_key(&sub1.app_authentication_key)
            .unwrap()
            .0,
        authentication1.as_bytes()
    );
    let sub2 = subs
        .iter()
        .find(|sub| sub.app_domain == app_domain2.domain())
        .unwrap();
    assert_eq!(sub2.scope, notification_types2);
    assert_eq!(sub2.account, account);
    assert_eq!(sub2.app_domain, app_domain2.domain());
    assert_eq!(sub2.app_authentication_key, client_id2.to_did_key());
    assert_eq!(
        &DecodedClientId::try_from_did_key(&sub2.app_authentication_key)
            .unwrap()
            .0,
        authentication2.as_bytes()
    );
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn this_domain_only(notify_server: &NotifyServerContext) {
    let (account_signing_key, account) = generate_account();

    let keys_server = MockServer::start().await;
    let keys_server_url = keys_server.uri().parse::<Url>().unwrap();

    let (identity_signing_key, identity_public_key) = generate_identity_key();
    let identity_key_details = IdentityKeyDetails {
        keys_server_url,
        signing_key: identity_signing_key,
        client_id: identity_public_key.clone(),
    };

    let project_id1 = ProjectId::generate();
    let app_domain1 = DidWeb::from_domain(format!("{project_id1}.example.com"));
    let (key_agreement1, _authentication1, client_id1) =
        subscribe_topic(&project_id1, app_domain1.clone(), &notify_server.url).await;

    register_mocked_identity_key(
        &keys_server,
        identity_public_key.clone(),
        sign_cacao(
            &app_domain1,
            &account,
            STATEMENT_THIS_DOMAIN.to_owned(),
            identity_public_key.clone(),
            identity_key_details.keys_server_url.to_string(),
            account_signing_key,
        ),
    )
    .await;

    let project_id2 = ProjectId::generate();
    let app_domain2 = DidWeb::from_domain(format!("{project_id2}.example.com"));
    let (key_agreement2, _authentication2, client_id2) =
        subscribe_topic(&project_id2, app_domain2.clone(), &notify_server.url).await;

    let vars = get_vars();
    let (relay_ws_client, mut rx) = create_client(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    let (subs, watch_topic_key, notify_server_client_id) = watch_subscriptions(
        notify_server.url.clone(),
        &identity_key_details,
        Some(app_domain1.clone()),
        &account,
        &relay_ws_client,
        &mut rx,
    )
    .await;
    assert!(subs.is_empty());

    let notification_type1 = Uuid::new_v4();
    let notification_types1 = HashSet::from([notification_type1, Uuid::new_v4()]);
    subscribe(
        &relay_ws_client,
        &mut rx,
        &account,
        &identity_key_details,
        key_agreement1,
        &client_id1,
        app_domain1.clone(),
        notification_types1.clone(),
    )
    .await;
    let subs = accept_watch_subscriptions_changed(
        &notify_server_client_id,
        &identity_key_details,
        &account,
        watch_topic_key,
        &relay_ws_client,
        &mut rx,
    )
    .await;
    assert_eq!(subs.len(), 1);
    let sub = &subs[0];
    assert_eq!(sub.account, account);
    assert_eq!(sub.app_domain, app_domain1.domain());

    let notification_type2 = Uuid::new_v4();
    let notification_types2 = HashSet::from([notification_type2, Uuid::new_v4()]);
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        subscribe(
            &relay_ws_client,
            &mut rx,
            &account,
            &identity_key_details,
            key_agreement2,
            &client_id2,
            app_domain2.clone(),
            notification_types2.clone(),
        ),
    )
    .await;
    assert!(result.is_err());
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        accept_watch_subscriptions_changed(
            &notify_server_client_id,
            &identity_key_details,
            &account,
            watch_topic_key,
            &relay_ws_client,
            &mut rx,
        ),
    )
    .await;
    assert!(result.is_err());
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn works_with_staging_keys_server(notify_server: &NotifyServerContext) {
    let (account_signing_key, account) = generate_account();

    let project_id = ProjectId::generate();
    let app_domain = DidWeb::from_domain(format!("{project_id}.example.com"));
    subscribe_topic(&project_id, app_domain.clone(), &notify_server.url).await;

    let keys_server_url = "https://staging.keys.walletconnect.com"
        .parse::<Url>()
        .unwrap();

    let (identity_signing_key, identity_public_key) = generate_identity_key();
    let identity_key_details = IdentityKeyDetails {
        keys_server_url,
        signing_key: identity_signing_key,
        client_id: identity_public_key.clone(),
    };

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
                    STATEMENT_THIS_DOMAIN.to_owned(),
                    identity_public_key.clone(),
                    identity_key_details.keys_server_url.to_string(),
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

    let (_subs, _watch_topic_key, _notify_server_client_id) = watch_subscriptions(
        notify_server.url.clone(),
        &identity_key_details,
        Some(app_domain),
        &account,
        &relay_ws_client,
        &mut rx,
    )
    .await;

    unregister_identity_key(
        identity_key_details.keys_server_url,
        &account,
        &identity_key_details.signing_key,
        &identity_public_key,
    )
    .await;
}

async fn setup_subscription(
    notify_server_url: Url,
    notification_types: HashSet<Uuid>,
) -> (
    Arc<Client>,
    UnboundedReceiver<RelayClientEvent>,
    AccountId,
    IdentityKeyDetails,
    ProjectId,
    DidWeb,
    DecodedClientId,
    [u8; 32],
) {
    let (account_signing_key, account) = generate_account();

    let keys_server = MockServer::start().await;
    let keys_server_url = keys_server.uri().parse::<Url>().unwrap();

    let (identity_signing_key, identity_public_key) = generate_identity_key();
    let identity_key_details = IdentityKeyDetails {
        keys_server_url,
        signing_key: identity_signing_key,
        client_id: identity_public_key.clone(),
    };

    let project_id = ProjectId::generate();
    let app_domain = DidWeb::from_domain(format!("{project_id}.walletconnect.com"));

    register_mocked_identity_key(
        &keys_server,
        identity_public_key.clone(),
        sign_cacao(
            &app_domain,
            &account,
            STATEMENT_THIS_DOMAIN.to_owned(),
            identity_public_key.clone(),
            identity_key_details.keys_server_url.to_string(),
            account_signing_key,
        ),
    )
    .await;

    let vars = get_vars();
    let (relay_ws_client, mut rx) = create_client(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server_url.clone(),
    )
    .await;

    let (key_agreement, _authentication, app_client_id) =
        subscribe_topic(&project_id, app_domain.clone(), &notify_server_url).await;

    let (subs, watch_topic_key, notify_server_client_id) = watch_subscriptions(
        notify_server_url,
        &identity_key_details,
        Some(app_domain.clone()),
        &account,
        &relay_ws_client,
        &mut rx,
    )
    .await;
    assert!(subs.is_empty());

    subscribe(
        &relay_ws_client,
        &mut rx,
        &account,
        &identity_key_details,
        key_agreement,
        &app_client_id,
        app_domain.clone(),
        notification_types,
    )
    .await;
    let subs = accept_watch_subscriptions_changed(
        &notify_server_client_id,
        &identity_key_details,
        &account,
        watch_topic_key,
        &relay_ws_client,
        &mut rx,
    )
    .await;
    assert_eq!(subs.len(), 1);
    let sub = &subs[0];

    let notify_key = decode_key(&sub.sym_key).unwrap();

    topic_subscribe(relay_ws_client.as_ref(), topic_from_key(&notify_key))
        .await
        .unwrap();

    (
        relay_ws_client,
        rx,
        account,
        identity_key_details,
        project_id,
        app_domain,
        app_client_id,
        notify_key,
    )
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn integration_get_notifications_has_none(notify_server: &NotifyServerContext) {
    let (
        relay_ws_client,
        mut rx,
        account,
        identity_key_details,
        _project_id,
        app_domain,
        app_client_id,
        notify_key,
    ) = setup_subscription(notify_server.url.clone(), HashSet::from([Uuid::new_v4()])).await;

    let result = get_notifications(
        &relay_ws_client,
        &mut rx,
        &account,
        &identity_key_details,
        &app_domain,
        &app_client_id,
        notify_key,
        GetNotificationsParams {
            limit: 5,
            after: None,
        },
    )
    .await;
    assert!(result.notifications.is_empty());
    assert!(!result.has_more);

    let failed_result = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        get_notifications(
            &relay_ws_client,
            &mut rx,
            &account,
            &identity_key_details,
            &app_domain,
            &app_client_id,
            notify_key,
            GetNotificationsParams {
                limit: 51, // larger than the maximum of 50
                after: None,
            },
        ),
    )
    .await;
    assert!(failed_result.is_err());
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn integration_get_notifications_has_one(notify_server: &NotifyServerContext) {
    let notification_type = Uuid::new_v4();
    let (
        relay_ws_client,
        mut rx,
        account,
        identity_key_details,
        project_id,
        app_domain,
        app_client_id,
        notify_key,
    ) = setup_subscription(
        notify_server.url.clone(),
        HashSet::from([notification_type]),
    )
    .await;

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

    assert_successful_response(
        reqwest::Client::new()
            .post(
                notify_server
                    .url
                    .join(&format!("{project_id}/notify"))
                    .unwrap(),
            )
            .bearer_auth(Uuid::new_v4())
            .json(&notify_body)
            .send()
            .await
            .unwrap(),
    )
    .await;

    let result = get_notifications(
        &relay_ws_client,
        &mut rx,
        &account,
        &identity_key_details,
        &app_domain,
        &app_client_id,
        notify_key,
        GetNotificationsParams {
            limit: 5,
            after: None,
        },
    )
    .await;
    assert_eq!(result.notifications.len(), 1);
    assert!(!result.has_more);

    let gotten_notification = &result.notifications[0];
    assert_eq!(notification.r#type, gotten_notification.r#type);
    assert_eq!(notification.title, gotten_notification.title);
    assert_eq!(notification.body, gotten_notification.body);
}

#[tokio::test]
async fn get_notifications_0() {
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
    let subscriber_topic = topic_from_key(&subscriber_sym_key);
    let subscriber_scope = HashSet::from([Uuid::new_v4(), Uuid::new_v4()]);
    let subscriber = upsert_subscriber(
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

    let result = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams {
            limit: 5,
            after: None,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert!(result.notifications.is_empty());
    assert!(!result.has_more);
}

#[tokio::test]
async fn get_notifications_1() {
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
    let subscriber_topic = topic_from_key(&subscriber_sym_key);
    let subscriber_scope = HashSet::from([Uuid::new_v4(), Uuid::new_v4()]);
    let subscriber = upsert_subscriber(
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

    let notification = Notification {
        r#type: Uuid::new_v4(),
        title: "title".to_owned(),
        body: "body".to_owned(),
        icon: None,
        url: None,
    };

    let notification_with_id = upsert_notification(
        Uuid::new_v4().to_string(),
        project.id,
        notification.clone(),
        &postgres,
        None,
    )
    .await
    .unwrap();

    upsert_subscriber_notifications(notification_with_id.id, &[subscriber], &postgres, None)
        .await
        .unwrap();

    let result = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams {
            limit: 5,
            after: None,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.notifications.len(), 1);
    assert!(!result.has_more);

    let gotten_notification = &result.notifications[0];
    assert_eq!(notification.r#type, gotten_notification.r#type);
    assert_eq!(notification.title, gotten_notification.title);
    assert_eq!(notification.body, gotten_notification.body);
}

#[tokio::test]
async fn get_notifications_4() {
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
    let subscriber_topic = topic_from_key(&subscriber_sym_key);
    let subscriber_scope = HashSet::from([Uuid::new_v4(), Uuid::new_v4()]);
    let subscriber = upsert_subscriber(
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

    let notification_titles = ["1", "2", "3", "4"];
    assert_eq!(notification_titles.len(), 4);

    for title in notification_titles {
        let notification = Notification {
            r#type: Uuid::new_v4(),
            title: title.to_owned(),
            body: "body".to_owned(),
            icon: None,
            url: None,
        };

        let notification_with_id = upsert_notification(
            Uuid::new_v4().to_string(),
            project.id,
            notification.clone(),
            &postgres,
            None,
        )
        .await
        .unwrap();

        upsert_subscriber_notifications(notification_with_id.id, &[subscriber], &postgres, None)
            .await
            .unwrap();
    }

    let result = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams {
            limit: 5,
            after: None,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.notifications.len(), notification_titles.len());
    assert!(!result.has_more);

    let mut gotten_ids = HashSet::with_capacity(7);
    let mut gotten_titles = HashSet::with_capacity(7);
    for notification in result.notifications {
        gotten_ids.insert(notification.id);
        gotten_titles.insert(notification.title);
    }

    assert_eq!(
        gotten_titles,
        notification_titles
            .into_iter()
            .map(|s| s.to_owned())
            .collect::<HashSet<_>>()
    );
    assert_eq!(gotten_ids.len(), 4);
}

#[tokio::test]
async fn get_notifications_5() {
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
    let subscriber_topic = topic_from_key(&subscriber_sym_key);
    let subscriber_scope = HashSet::from([Uuid::new_v4(), Uuid::new_v4()]);
    let subscriber = upsert_subscriber(
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

    let notification_titles = ["1", "2", "3", "4", "5"];
    assert_eq!(notification_titles.len(), 5);

    for title in notification_titles {
        let notification = Notification {
            r#type: Uuid::new_v4(),
            title: title.to_owned(),
            body: "body".to_owned(),
            icon: None,
            url: None,
        };

        let notification_with_id = upsert_notification(
            Uuid::new_v4().to_string(),
            project.id,
            notification.clone(),
            &postgres,
            None,
        )
        .await
        .unwrap();

        upsert_subscriber_notifications(notification_with_id.id, &[subscriber], &postgres, None)
            .await
            .unwrap();
    }

    let limit = 5;

    let result = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams { limit, after: None },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.notifications.len(), limit);
    assert_eq!(result.notifications.len(), notification_titles.len());
    assert!(!result.has_more);

    let mut gotten_ids = HashSet::with_capacity(7);
    let mut gotten_titles = HashSet::with_capacity(7);
    for notification in result.notifications {
        gotten_ids.insert(notification.id);
        gotten_titles.insert(notification.title);
    }

    assert_eq!(
        gotten_titles,
        notification_titles
            .into_iter()
            .map(|s| s.to_owned())
            .collect::<HashSet<_>>()
    );
    assert_eq!(gotten_ids.len(), 5);
}

#[tokio::test]
async fn get_notifications_6() {
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
    let subscriber_topic = topic_from_key(&subscriber_sym_key);
    let subscriber_scope = HashSet::from([Uuid::new_v4(), Uuid::new_v4()]);
    let subscriber = upsert_subscriber(
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

    let notification_titles = ["1", "2", "3", "4", "5", "6"];
    assert_eq!(notification_titles.len(), 6);

    for title in notification_titles {
        let notification = Notification {
            r#type: Uuid::new_v4(),
            title: title.to_owned(),
            body: "body".to_owned(),
            icon: None,
            url: None,
        };

        let notification_with_id = upsert_notification(
            Uuid::new_v4().to_string(),
            project.id,
            notification.clone(),
            &postgres,
            None,
        )
        .await
        .unwrap();

        upsert_subscriber_notifications(notification_with_id.id, &[subscriber], &postgres, None)
            .await
            .unwrap();
    }

    let limit = 5;

    let mut gotten_ids = HashSet::with_capacity(6);
    let mut gotten_titles = HashSet::with_capacity(6);

    let first_page = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams { limit, after: None },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(first_page.notifications.len(), limit);
    assert!(first_page.has_more);

    let last_id = first_page.notifications.last().unwrap().id;
    for notification in first_page.notifications {
        gotten_ids.insert(notification.id);
        gotten_titles.insert(notification.title);
    }

    let second_page = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams {
            limit,
            after: Some(last_id),
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert!(second_page.notifications.len() < limit);
    assert_eq!(second_page.notifications.len(), 1);
    assert!(!second_page.has_more);

    for notification in second_page.notifications {
        gotten_ids.insert(notification.id);
        gotten_titles.insert(notification.title);
    }

    assert_eq!(
        gotten_titles,
        notification_titles
            .into_iter()
            .map(|s| s.to_owned())
            .collect::<HashSet<_>>()
    );
    assert_eq!(gotten_ids.len(), 6);
}

#[tokio::test]
async fn get_notifications_7() {
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
    let subscriber_topic = topic_from_key(&subscriber_sym_key);
    let subscriber_scope = HashSet::from([Uuid::new_v4(), Uuid::new_v4()]);
    let subscriber = upsert_subscriber(
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

    let notification_titles = ["1", "2", "3", "4", "5", "6", "7"];
    assert_eq!(notification_titles.len(), 7);

    for title in notification_titles {
        let notification = Notification {
            r#type: Uuid::new_v4(),
            title: title.to_owned(),
            body: "body".to_owned(),
            icon: None,
            url: None,
        };

        let notification_with_id = upsert_notification(
            Uuid::new_v4().to_string(),
            project.id,
            notification.clone(),
            &postgres,
            None,
        )
        .await
        .unwrap();

        upsert_subscriber_notifications(notification_with_id.id, &[subscriber], &postgres, None)
            .await
            .unwrap();
    }

    let limit = 5;

    let first_page = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams { limit, after: None },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(first_page.notifications.len(), limit);
    assert!(first_page.has_more);

    for (notification, expected_title) in first_page
        .notifications
        .iter()
        .zip(notification_titles[0..limit].iter())
    {
        assert_eq!(&notification.title, expected_title);
    }

    let second_page = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams {
            limit,
            after: Some(first_page.notifications.last().unwrap().id),
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert!(second_page.notifications.len() < limit);
    assert_eq!(second_page.notifications.len(), 2);
    assert!(!second_page.has_more);

    assert_eq!(&second_page.notifications[0].title, notification_titles[5]);
    assert_eq!(&second_page.notifications[1].title, notification_titles[6]);

    // TODO apply HashMap approach for all of these?
}

#[tokio::test]
async fn different_created_at() {
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
    let subscriber_topic = topic_from_key(&subscriber_sym_key);
    let subscriber_scope = HashSet::from([Uuid::new_v4(), Uuid::new_v4()]);
    let subscriber = upsert_subscriber(
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

    let notification_titles = ["1", "2", "3", "4", "5", "6", "7"];
    assert_eq!(notification_titles.len(), 7);

    for title in notification_titles {
        let notification = Notification {
            r#type: Uuid::new_v4(),
            title: title.to_owned(),
            body: "body".to_owned(),
            icon: None,
            url: None,
        };

        let notification_with_id = upsert_notification(
            Uuid::new_v4().to_string(),
            project.id,
            notification.clone(),
            &postgres,
            None,
        )
        .await
        .unwrap();

        upsert_subscriber_notifications(notification_with_id.id, &[subscriber], &postgres, None)
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    let limit = 5;

    let first_page = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams { limit, after: None },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(first_page.notifications.len(), limit);
    assert!(first_page.has_more);

    for (notification, expected_title) in first_page
        .notifications
        .iter()
        .zip(notification_titles[0..limit].iter())
    {
        assert_eq!(&notification.title, expected_title);
    }

    let second_page = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams {
            limit,
            after: Some(first_page.notifications.last().unwrap().id),
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert!(second_page.notifications.len() < limit);
    assert_eq!(second_page.notifications.len(), 2);
    assert!(!second_page.has_more);

    assert_eq!(&second_page.notifications[0].title, notification_titles[5]);
    assert_eq!(&second_page.notifications[1].title, notification_titles[6]);
}

#[tokio::test]
async fn duplicate_created_at() {
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
    let subscriber_topic = topic_from_key(&subscriber_sym_key);
    let subscriber_scope = HashSet::from([Uuid::new_v4(), Uuid::new_v4()]);
    let subscriber = upsert_subscriber(
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

    let notification_titles = (1..=7).map(|i| i.to_string()).collect::<Vec<_>>();
    assert_eq!(notification_titles.len(), 7);

    let now = Utc::now();

    for title in &notification_titles {
        let notification = Notification {
            r#type: Uuid::new_v4(),
            title: title.to_owned(),
            body: "body".to_owned(),
            icon: None,
            url: None,
        };

        let notification_with_id = upsert_notification(
            Uuid::new_v4().to_string(),
            project.id,
            notification.clone(),
            &postgres,
            None,
        )
        .await
        .unwrap();

        let query = "
            INSERT INTO subscriber_notification (notification, subscriber, status)
            SELECT $1 AS notification, subscriber, $3::subscriber_notification_status FROM UNNEST($2) AS subscriber
        ";
        sqlx::query(query)
            .bind(notification_with_id.id)
            .bind([subscriber])
            .bind(SubscriberNotificationStatus::Queued.to_string())
            .bind(now)
            .execute(&postgres)
            .await
            .unwrap();
    }

    let limit = 5;

    let mut gotten_ids = HashSet::with_capacity(7);
    let mut gotten_titles = HashSet::with_capacity(7);

    let first_page = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams { limit, after: None },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(first_page.notifications.len(), limit);
    assert!(first_page.has_more);

    let last_id = first_page.notifications.last().unwrap().id;

    for notifiction in first_page.notifications {
        assert!(gotten_ids.insert(notifiction.id));
        assert!(gotten_titles.insert(notifiction.title));
    }

    let second_page = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams {
            limit,
            after: Some(last_id),
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert!(second_page.notifications.len() < limit);
    assert_eq!(second_page.notifications.len(), 2);
    assert!(!second_page.has_more);

    for notifiction in second_page.notifications {
        assert!(gotten_ids.insert(notifiction.id));
        assert!(gotten_titles.insert(notifiction.title));
    }

    let notification_titles = notification_titles.into_iter().collect::<HashSet<_>>();
    assert_eq!(notification_titles.len(), 7);
    assert_eq!(gotten_titles.len(), 7);
    assert_eq!(notification_titles, gotten_titles);
    assert_eq!(gotten_ids.len(), 7);
}

// TODO test deleting and re-subscribing
