use {
    crate::utils::{
        assert_successful_response, encode_auth,
        notify_relay_api::{accept_watch_subscriptions_changed, subscribe, watch_subscriptions},
        relay_api::{decode_message, decode_response_message},
        RelayClient, RELAY_MESSAGE_DELIVERY_TIMEOUT,
    },
    async_trait::async_trait,
    chrono::{DateTime, Duration, Utc},
    futures::future::BoxFuture,
    futures_util::StreamExt,
    hyper::StatusCode,
    itertools::Itertools,
    notify_server::{
        auth::{
            encode_authentication_private_key, encode_authentication_public_key,
            encode_subscribe_private_key, encode_subscribe_public_key, from_jwt,
            test_utils::{
                generate_identity_key, register_mocked_identity_key, sign_cacao, CacaoAuth,
                IdentityKeyDetails,
            },
            CacaoValue, DidWeb, GetSharedClaims, MessageResponseAuth, NotifyServerSubscription,
            SubscriptionDeleteRequestAuth, SubscriptionDeleteResponseAuth,
            SubscriptionGetNotificationsRequestAuth, SubscriptionGetNotificationsResponseAuth,
            SubscriptionMarkNotificationsAsReadRequestAuth,
            SubscriptionMarkNotificationsAsReadResponseAuth, SubscriptionUpdateRequestAuth,
            SubscriptionUpdateResponseAuth, STATEMENT_ALL_DOMAINS, STATEMENT_THIS_DOMAIN,
        },
        config::Configuration,
        model::{
            helpers::{
                get_notifications_for_subscriber, get_project_by_app_domain,
                get_project_by_project_id, get_project_by_topic, get_project_topics,
                get_subscriber_accounts_by_project_id, get_subscriber_by_topic,
                get_subscriber_topics, get_subscribers_by_project_id_and_accounts,
                get_subscribers_for_project_in, get_subscriptions_by_account_and_maybe_app,
                get_welcome_notification, mark_all_notifications_as_read_for_project,
                mark_notifications_as_read, set_welcome_notification, upsert_project,
                upsert_subscriber, GetNotificationsParams, GetNotificationsResult,
                MarkNotificationsAsReadParams, SubscribeResponse, SubscriberAccountAndScopes,
                WelcomeNotification,
            },
            types::{
                eip155::test_utils::{format_eip155_account, generate_account, generate_eoa},
                AccountId,
            },
        },
        notify_message::NotifyMessage,
        rate_limit::{self, ClockImpl},
        registry::{storage::redis::Redis, RegistryAuthResponse},
        relay_client_helpers::create_http_client,
        rpc::{
            decode_key, AuthMessage, JsonRpcRequest, JsonRpcResponse, JsonRpcResponseError,
            NotifyDelete, NotifyUpdate, ResponseAuth,
        },
        services::{
            public_http_server::{
                handlers::{
                    get_subscribers_v1::{
                        GetSubscribersBody, GetSubscribersResponse, GetSubscribersResponseEntry,
                    },
                    notification_link::format_follow_link,
                    notify_v0::NotifyBody,
                    notify_v1::{
                        self, notify_rate_limit, subscriber_rate_limit, subscriber_rate_limit_key,
                        NotifyBodyNotification,
                    },
                    relay_webhook::handlers::notify_watch_subscriptions::SUBSCRIPTION_WATCHER_LIMIT,
                    subscribe_topic::{SubscribeTopicRequestBody, SubscribeTopicResponseBody},
                },
                RELAY_WEBHOOK_ENDPOINT,
            },
            publisher_service::{
                helpers::{
                    dead_letter_give_up_check, dead_letters_check,
                    pick_subscriber_notification_for_processing, upsert_notification,
                    upsert_subscriber_notifications, NotificationToProcess,
                },
                types::SubscriberNotificationStatus,
                NOTIFICATION_FOR_DELIVERY,
            },
            relay_mailbox_clearing_service::BATCH_TIMEOUT,
        },
        spec::{
            NOTIFY_DELETE_ACT, NOTIFY_DELETE_METHOD, NOTIFY_DELETE_RESPONSE_ACT,
            NOTIFY_DELETE_RESPONSE_TAG, NOTIFY_DELETE_TAG, NOTIFY_DELETE_TTL,
            NOTIFY_GET_NOTIFICATIONS_ACT, NOTIFY_GET_NOTIFICATIONS_RESPONSE_ACT,
            NOTIFY_GET_NOTIFICATIONS_RESPONSE_TAG, NOTIFY_GET_NOTIFICATIONS_TAG,
            NOTIFY_GET_NOTIFICATIONS_TTL, NOTIFY_MARK_NOTIFICATIONS_AS_READ_ACT,
            NOTIFY_MARK_NOTIFICATIONS_AS_READ_METHOD,
            NOTIFY_MARK_NOTIFICATIONS_AS_READ_RESPONSE_ACT,
            NOTIFY_MARK_NOTIFICATIONS_AS_READ_RESPONSE_TAG, NOTIFY_MARK_NOTIFICATIONS_AS_READ_TAG,
            NOTIFY_MARK_NOTIFICATIONS_AS_READ_TTL, NOTIFY_MESSAGE_RESPONSE_ACT,
            NOTIFY_MESSAGE_RESPONSE_TAG, NOTIFY_MESSAGE_RESPONSE_TTL, NOTIFY_UPDATE_ACT,
            NOTIFY_UPDATE_METHOD, NOTIFY_UPDATE_RESPONSE_ACT, NOTIFY_UPDATE_RESPONSE_TAG,
            NOTIFY_UPDATE_TAG, NOTIFY_UPDATE_TTL,
        },
        types::{encode_scope, Notification},
        utils::{get_client_id, is_same_address, topic_from_key},
    },
    rand::{rngs::StdRng, SeedableRng},
    rand_chacha::rand_core::OsRng,
    relay_rpc::{
        auth::ed25519_dalek::{Signer, SigningKey, VerifyingKey},
        domain::{DecodedClientId, DidKey, MessageId, ProjectId, Topic},
        jwt::{JwtBasicClaims, JwtHeader, VerifyableClaims},
        rpc::{
            SubscriptionData, WatchAction, WatchEventClaims, WatchEventPayload, WatchStatus,
            WatchType, WatchWebhookPayload,
        },
    },
    reqwest::redirect,
    serde::de::DeserializeOwned,
    serde_json::{json, Value},
    sqlx::{
        error::BoxDynError,
        migrate::{Migration, MigrationSource, Migrator},
        postgres::PgPoolOptions,
        FromRow, PgPool, Postgres,
    },
    std::{
        collections::{HashMap, HashSet},
        env,
        net::{IpAddr, Ipv4Addr, SocketAddr},
        ops::AddAssign,
        sync::{Arc, RwLock},
    },
    test_context::{test_context, AsyncTestContext},
    tokio::{
        net::{TcpListener, ToSocketAddrs},
        sync::broadcast,
        time::error::Elapsed,
    },
    tracing_subscriber::fmt::format::FmtSpan,
    url::Url,
    utils::{
        notify_relay_api::{accept_notify_message, subscribe_with_mjv},
        relay_api::{publish_jwt_message, TopicEncrptionScheme},
        unregister_identity_key,
    },
    uuid::Uuid,
    wiremock::MockServer,
    x25519_dalek::PublicKey,
};

mod utils;

// Unit-like integration tests able to be run locally with minimal configuration; only relay project ID is required.
// Simply initialize .env with the integration configuration and run `just test-integration`

// The only variable that's needed is a valid relay project ID because the relay is not mocked.
// The registry is mocked out, so any project ID or notify secret is valid and are generated randomly in these tests.
// The prod relay will always be used, to allow tests to run longer than 1 minute and enabling debugging with data lake
// The localhost Postgres will always be used. This is valid in both docker-compose.storage and GitHub CI.

// TODO make these DRY with local configuration defaults
fn get_vars() -> Vars {
    Vars {
        project_id: env::var("PROJECT_ID").unwrap(),

        // No use-case to modify these currently.
        relay_url: "http://127.0.0.1:8888".to_owned(),
    }
}

struct Vars {
    project_id: String,
    relay_url: String,
}

async fn get_postgres_without_migration() -> (PgPool, String) {
    let base_url = "postgres://postgres:password@localhost:5432";

    let postgres = PgPoolOptions::new()
        .acquire_timeout(std::time::Duration::from_secs(60))
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

    (postgres, postgres_url)
}

async fn get_postgres() -> (PgPool, String) {
    let (postgres, postgres_url) = get_postgres_without_migration().await;
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

fn generate_authentication_key() -> SigningKey {
    SigningKey::generate(&mut OsRng)
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

    let subscribers =
        get_subscriptions_by_account_and_maybe_app(account_id.clone(), None, &postgres, None)
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

    let account_id2 = generate_account_id();
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

    let subscribers =
        get_subscriptions_by_account_and_maybe_app(account_id.clone(), None, &postgres, None)
            .await
            .unwrap();
    assert_eq!(subscribers.len(), 1);
    let subscriber = &subscribers[0];
    assert_eq!(subscriber.app_domain, project.app_domain);
    assert_eq!(subscriber.account, account_id);
    assert_eq!(subscriber.sym_key, hex::encode(subscriber_sym_key));
    assert_eq!(subscriber.scope, subscriber_scope);
    assert!(subscriber.expiry > Utc::now() + Duration::days(29));

    let subscribers =
        get_subscriptions_by_account_and_maybe_app(account_id2.clone(), None, &postgres, None)
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

    let subscribers =
        get_subscriptions_by_account_and_maybe_app(account_id.clone(), None, &postgres, None)
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

    let subscribers =
        get_subscribers_by_project_id_and_accounts(project_id, &[account.clone()], &postgres, None)
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
    time::timeout(Duration::from_secs(10), async {
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
    keypair_seed: String,
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
        let relay_url = vars.relay_url.parse::<Url>().unwrap();
        let relay_public_key = reqwest::get(relay_url.join("/public-key").unwrap())
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        let (_, postgres_url) = get_postgres().await;
        let keypair_seed = hex::encode(rand::Rng::gen::<[u8; 10]>(&mut rand::thread_rng()));
        let clock = Arc::new(MockClock::new(Utc::now()));
        // TODO reuse the local configuration defaults here
        let config = Configuration {
            postgres_url,
            postgres_max_connections: 2,
            log_level: "WARN,notify_server=DEBUG".to_string(),
            public_ip: bind_ip,
            bind_ip,
            port: bind_port,
            registry_url: registry_mock_server.uri().parse().unwrap(),
            keypair_seed: keypair_seed.clone(),
            project_id: vars.project_id.into(),
            relay_url,
            relay_public_key,
            notify_url: notify_url.clone(),
            blockchain_api_endpoint: None,
            registry_auth_token: "".to_owned(),
            auth_redis_addr_read: Some("redis://localhost:6378/0".to_owned()),
            auth_redis_addr_write: Some("redis://localhost:6378/0".to_owned()),
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
            .acquire_timeout(std::time::Duration::from_secs(60))
            .max_connections(1)
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
            keypair_seed,
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
    let subscribers = assert_successful_response(
        reqwest::Client::new()
            .post(
                notify_server
                    .url
                    .join(&format!("/v1/{project_id}/subscribers"))
                    .unwrap(),
            )
            .header("Authorization", format!("Bearer {}", Uuid::new_v4()))
            .json(&GetSubscribersBody {
                accounts: vec![account.clone()],
            })
            .send()
            .await
            .unwrap(),
    )
    .await
    .json::<GetSubscribersResponse>()
    .await
    .unwrap();
    assert_eq!(subscribers, HashMap::new());

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

    let subscribers = get_subscribers_by_project_id_and_accounts(
        project_id.clone(),
        &[account.clone()],
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
            .post(
                notify_server
                    .url
                    .join(&format!("/v1/{project_id}/subscribers"))
                    .unwrap(),
            )
            .header("Authorization", format!("Bearer {}", Uuid::new_v4()))
            .json(&GetSubscribersBody {
                accounts: vec![account.clone()],
            })
            .send()
            .await
            .unwrap(),
    )
    .await
    .json::<GetSubscribersResponse>()
    .await
    .unwrap();
    assert_eq!(
        subscribers,
        HashMap::from([(
            account,
            GetSubscribersResponseEntry {
                notification_types: scope
            }
        )])
    );
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn test_get_subscribers_v1_empty_response(notify_server: &NotifyServerContext) {
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

    let accounts = (0..2).map(|_| generate_account_id()).collect();
    let subscribers = assert_successful_response(
        reqwest::Client::new()
            .post(
                notify_server
                    .url
                    .join(&format!("/v1/{project_id}/subscribers"))
                    .unwrap(),
            )
            .header("Authorization", format!("Bearer {}", Uuid::new_v4()))
            .json(&GetSubscribersBody { accounts })
            .send()
            .await
            .unwrap(),
    )
    .await
    .json::<GetSubscribersResponse>()
    .await
    .unwrap();
    assert_eq!(subscribers, HashMap::new());
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn test_get_subscribers_v1_len_check(notify_server: &NotifyServerContext) {
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

    let accounts = (0..100).map(|_| generate_account_id()).collect();
    let subscribers = assert_successful_response(
        reqwest::Client::new()
            .post(
                notify_server
                    .url
                    .join(&format!("/v1/{project_id}/subscribers"))
                    .unwrap(),
            )
            .header("Authorization", format!("Bearer {}", Uuid::new_v4()))
            .json(&GetSubscribersBody { accounts })
            .send()
            .await
            .unwrap(),
    )
    .await
    .json::<GetSubscribersResponse>()
    .await
    .unwrap();
    assert_eq!(subscribers, HashMap::new());

    let response = reqwest::Client::new()
        .post(
            notify_server
                .url
                .join(&format!("/v1/{project_id}/subscribers"))
                .unwrap(),
        )
        .header("Authorization", format!("Bearer {}", Uuid::new_v4()))
        .json(&GetSubscribersBody { accounts: vec![] })
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let response = response.text().await.unwrap();
    assert!(response.contains("accounts: Validation error: length"));

    let accounts = (0..101).map(|_| generate_account_id()).collect();
    let response = reqwest::Client::new()
        .post(
            notify_server
                .url
                .join(&format!("/v1/{project_id}/subscribers"))
                .unwrap(),
        )
        .header("Authorization", format!("Bearer {}", Uuid::new_v4()))
        .json(&GetSubscribersBody { accounts })
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let response = response.text().await.unwrap();
    assert!(response.contains("accounts: Validation error: length"));
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
    let mut relay_client = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    relay_client.subscribe(notify_topic).await;

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
        &mut relay_client,
        &account,
        &authentication_key.verifying_key(),
        &get_client_id(&authentication_key.verifying_key()),
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
    let mut relay_client = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    relay_client.subscribe(notify_topic).await;

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
    .json::<notify_v1::ResponseBody>()
    .await
    .unwrap();
    assert!(response.not_found.is_empty());
    assert!(response.failed.is_empty());
    assert_eq!(response.sent, HashSet::from([account.clone()]));

    let (_, claims) = accept_notify_message(
        &mut relay_client,
        &account,
        &authentication_key.verifying_key(),
        &get_client_id(&authentication_key.verifying_key()),
        &app_domain,
        &notify_key,
    )
    .await
    .unwrap();

    assert_eq!(claims.msg.r#type, notification.r#type);
    assert_eq!(claims.msg.title, notification.title);
    assert_eq!(claims.msg.body, notification.body);
    assert_eq!(Some(&claims.msg.icon), notification.icon.as_ref());
    assert_ne!(claims.msg.url, "");
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn test_notify_v0_only_required_fields(notify_server: &NotifyServerContext) {
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
    let mut relay_client = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    relay_client.subscribe(notify_topic).await;

    let notify_body = json!({
        "notification": {
            "type": notification_type,
            "title": "title",
            "body": "body",
        },
        "accounts": [account.clone()]
    });

    let response = assert_successful_response(
        reqwest::Client::new()
            .post(
                notify_server
                    .url
                    .join(&format!("/{project_id}/notify"))
                    .unwrap(),
            )
            .bearer_auth(Uuid::new_v4())
            .json(&notify_body)
            .send()
            .await
            .unwrap(),
    )
    .await
    .json::<notify_v1::ResponseBody>()
    .await
    .unwrap();
    assert!(response.not_found.is_empty());
    assert!(response.failed.is_empty());
    assert_eq!(response.sent, HashSet::from([account.clone()]));

    let (_, claims) = accept_notify_message(
        &mut relay_client,
        &account,
        &authentication_key.verifying_key(),
        &get_client_id(&authentication_key.verifying_key()),
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
    .json::<notify_v1::ResponseBody>()
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
    .json::<notify_v1::ResponseBody>()
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
            let refill_in = DateTime::from_timestamp_millis(result.1 as i64)
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
    let SubscribeResponse {
        id: subscriber_id, ..
    } = upsert_subscriber(
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
        .json::<notify_v1::ResponseBody>()
        .await
        .unwrap();
    assert!(response.not_found.is_empty());
    assert!(response.failed.is_empty());
    assert_eq!(response.sent, HashSet::from([account.clone()]));

    let response = assert_successful_response(notify().await)
        .await
        .json::<notify_v1::ResponseBody>()
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
    let SubscribeResponse {
        id: subscriber_id1, ..
    } = upsert_subscriber(
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
    let SubscribeResponse {
        id: _subscriber_id2,
        ..
    } = upsert_subscriber(
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
        .json::<notify_v1::ResponseBody>()
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
        .json::<notify_v1::ResponseBody>()
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
    let SubscribeResponse { id: subscriber, .. } = upsert_subscriber(
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

    let subscribers = get_subscribers_by_project_id_and_accounts(
        project_id.clone(),
        &[account.clone()],
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

    let subscribers = get_subscribers_by_project_id_and_accounts(
        project_id.clone(),
        &[account.clone()],
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
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
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
async fn test_notify_invalid_account(notify_server: &NotifyServerContext) {
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

    let (_, address) = generate_eoa();

    let notify_body = json!([{
        "notification": {
            "type": Uuid::new_v4(),
            "title": "title",
            "body": "body",
        },
        "accounts": [address]
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
    assert!(response.contains("Account ID is is not a valid CAIP-10 account ID"));
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
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
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
    let SubscribeResponse {
        id: subscriber_id, ..
    } = upsert_subscriber(
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
    pg_listener.listen(NOTIFICATION_FOR_DELIVERY).await.unwrap();
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

#[allow(clippy::too_many_arguments)]
pub async fn subscribe_v1(
    relay_client: &mut RelayClient,
    account: &AccountId,
    identity_key_details: &IdentityKeyDetails,
    app_key_agreement_key: x25519_dalek::PublicKey,
    app_client_id: &DecodedClientId,
    app: DidWeb,
    notification_types: HashSet<Uuid>,
) -> Vec<NotifyServerSubscription> {
    subscribe_with_mjv(
        relay_client,
        account,
        identity_key_details,
        app_key_agreement_key,
        app_client_id,
        app,
        notification_types,
        "1".to_owned(),
    )
    .await
    .unwrap()
}

pub fn decode_auth_message<T>(
    msg: SubscriptionData,
    key: &[u8; 32],
) -> Result<(MessageId, T), JsonRpcResponseError>
where
    T: GetSharedClaims + DeserializeOwned,
{
    let response = decode_message::<JsonRpcResponse<AuthMessage>>(msg, key);
    match response {
        Ok(response) => Ok((response.id, from_jwt::<T>(&response.result.auth).unwrap())),
        Err(e) => Err(e),
    }
}

async fn publish_notify_message_response(
    relay_client: &mut RelayClient,
    account: &AccountId,
    app_client_id: &DecodedClientId,
    did_web: DidWeb,
    identity_key_details: &IdentityKeyDetails,
    sym_key: [u8; 32],
    id: MessageId,
) {
    publish_jwt_message(
        relay_client,
        app_client_id,
        identity_key_details,
        &TopicEncrptionScheme::Symetric(sym_key),
        NOTIFY_MESSAGE_RESPONSE_TAG,
        NOTIFY_MESSAGE_RESPONSE_TTL,
        NOTIFY_MESSAGE_RESPONSE_ACT,
        None,
        |shared_claims| {
            serde_json::to_value(JsonRpcResponse::new(
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
async fn accept_and_respond_to_notify_message(
    relay_client: &mut RelayClient,
    identity_key_details: &IdentityKeyDetails,
    account: &AccountId,
    app_authentication: &VerifyingKey,
    app_client_id: &DecodedClientId,
    app_domain: DidWeb,
    notify_key: [u8; 32],
) -> NotifyMessage {
    let (request_id, claims) = accept_notify_message(
        relay_client,
        account,
        app_authentication,
        app_client_id,
        &app_domain,
        &notify_key,
    )
    .await
    .unwrap();

    publish_notify_message_response(
        relay_client,
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

#[allow(clippy::too_many_arguments)]
async fn publish_update_request(
    relay_client: &mut RelayClient,
    account: &AccountId,
    client_id: &DecodedClientId,
    identity_key_details: &IdentityKeyDetails,
    sym_key: [u8; 32],
    app: DidWeb,
    notification_types: &HashSet<Uuid>,
    mjv: String,
) {
    publish_jwt_message(
        relay_client,
        client_id,
        identity_key_details,
        &TopicEncrptionScheme::Symetric(sym_key),
        NOTIFY_UPDATE_TAG,
        NOTIFY_UPDATE_TTL,
        NOTIFY_UPDATE_ACT,
        Some(mjv),
        |shared_claims| {
            serde_json::to_value(JsonRpcRequest::new(
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
    relay_client: &mut RelayClient,
    account: &AccountId,
    identity_key_details: &IdentityKeyDetails,
    app: &DidWeb,
    app_client_id: &DecodedClientId,
    notify_key: [u8; 32],
    notification_types: &HashSet<Uuid>,
) {
    let _subs = update_with_mjv(
        relay_client,
        account,
        identity_key_details,
        app,
        app_client_id,
        notify_key,
        notification_types,
        "0".to_owned(),
    )
    .await;
}

#[allow(clippy::too_many_arguments)]
async fn update_v1(
    relay_client: &mut RelayClient,
    account: &AccountId,
    identity_key_details: &IdentityKeyDetails,
    app: &DidWeb,
    app_client_id: &DecodedClientId,
    notify_key: [u8; 32],
    notification_types: &HashSet<Uuid>,
) -> Vec<NotifyServerSubscription> {
    update_with_mjv(
        relay_client,
        account,
        identity_key_details,
        app,
        app_client_id,
        notify_key,
        notification_types,
        "1".to_owned(),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn update_with_mjv(
    relay_client: &mut RelayClient,
    account: &AccountId,
    identity_key_details: &IdentityKeyDetails,
    app: &DidWeb,
    app_client_id: &DecodedClientId,
    notify_key: [u8; 32],
    notification_types: &HashSet<Uuid>,
    mjv: String,
) -> Vec<NotifyServerSubscription> {
    publish_update_request(
        relay_client,
        account,
        app_client_id,
        identity_key_details,
        notify_key,
        app.clone(),
        notification_types,
        mjv,
    )
    .await;

    let msg = relay_client
        .accept_message(NOTIFY_UPDATE_RESPONSE_TAG, &topic_from_key(&notify_key))
        .await;

    let (_id, auth) =
        decode_response_message::<SubscriptionUpdateResponseAuth>(msg, &notify_key).unwrap();
    assert_eq!(auth.shared_claims.act, NOTIFY_UPDATE_RESPONSE_ACT);
    assert_eq!(auth.shared_claims.iss, app_client_id.to_did_key());
    assert_eq!(
        auth.shared_claims.aud,
        identity_key_details.client_id.to_did_key()
    );
    assert_eq!(&auth.app, app);
    assert_eq!(auth.sub, account.to_did_pkh());

    auth.sbs
}

async fn publish_delete_request(
    relay_client: &mut RelayClient,
    account: &AccountId,
    client_id: &DecodedClientId,
    identity_key_details: &IdentityKeyDetails,
    sym_key: [u8; 32],
    app: DidWeb,
    mjv: String,
) {
    publish_jwt_message(
        relay_client,
        client_id,
        identity_key_details,
        &TopicEncrptionScheme::Symetric(sym_key),
        NOTIFY_DELETE_TAG,
        NOTIFY_DELETE_TTL,
        NOTIFY_DELETE_ACT,
        Some(mjv),
        |shared_claims| {
            serde_json::to_value(JsonRpcRequest::new(
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
    relay_client: &mut RelayClient,
    identity_key_details: &IdentityKeyDetails,
    app: &DidWeb,
    app_client_id: &DecodedClientId,
    account: &AccountId,
    notify_key: [u8; 32],
) {
    let _subs = delete_with_mjv(
        relay_client,
        identity_key_details,
        app,
        app_client_id,
        account,
        notify_key,
        "0".to_owned(),
    )
    .await;
}

#[allow(clippy::too_many_arguments)]
async fn delete_v1(
    relay_client: &mut RelayClient,
    identity_key_details: &IdentityKeyDetails,
    app: &DidWeb,
    app_client_id: &DecodedClientId,
    account: &AccountId,
    notify_key: [u8; 32],
) -> Vec<NotifyServerSubscription> {
    delete_with_mjv(
        relay_client,
        identity_key_details,
        app,
        app_client_id,
        account,
        notify_key,
        "1".to_owned(),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn delete_with_mjv(
    relay_client: &mut RelayClient,
    identity_key_details: &IdentityKeyDetails,
    app: &DidWeb,
    app_client_id: &DecodedClientId,
    account: &AccountId,
    notify_key: [u8; 32],
    mjv: String,
) -> Vec<NotifyServerSubscription> {
    publish_delete_request(
        relay_client,
        account,
        app_client_id,
        identity_key_details,
        notify_key,
        app.clone(),
        mjv,
    )
    .await;

    let msg = relay_client
        .accept_message(NOTIFY_DELETE_RESPONSE_TAG, &topic_from_key(&notify_key))
        .await;

    let (_id, auth) =
        decode_response_message::<SubscriptionDeleteResponseAuth>(msg, &notify_key).unwrap();
    assert_eq!(auth.shared_claims.act, NOTIFY_DELETE_RESPONSE_ACT);
    assert_eq!(auth.shared_claims.iss, app_client_id.to_did_key());
    assert_eq!(
        auth.shared_claims.aud,
        identity_key_details.client_id.to_did_key()
    );
    assert_eq!(&auth.app, app);
    assert_eq!(auth.sub, account.to_did_pkh());

    auth.sbs
}

async fn publish_get_notifications_request(
    relay_client: &mut RelayClient,
    account: &AccountId,
    client_id: &DecodedClientId,
    identity_key_details: &IdentityKeyDetails,
    sym_key: [u8; 32],
    app: DidWeb,
    params: GetNotificationsParams,
) {
    publish_jwt_message(
        relay_client,
        client_id,
        identity_key_details,
        &TopicEncrptionScheme::Symetric(sym_key),
        NOTIFY_GET_NOTIFICATIONS_TAG,
        NOTIFY_GET_NOTIFICATIONS_TTL,
        NOTIFY_GET_NOTIFICATIONS_ACT,
        None,
        |shared_claims| {
            serde_json::to_value(JsonRpcRequest::new(
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
    relay_client: &mut RelayClient,
    account: &AccountId,
    identity_key_details: &IdentityKeyDetails,
    app: &DidWeb,
    app_client_id: &DecodedClientId,
    notify_key: [u8; 32],
    params: GetNotificationsParams,
) -> Result<GetNotificationsResult, JsonRpcResponseError> {
    publish_get_notifications_request(
        relay_client,
        account,
        app_client_id,
        identity_key_details,
        notify_key,
        app.clone(),
        params,
    )
    .await;

    let msg = relay_client
        .accept_message(
            NOTIFY_GET_NOTIFICATIONS_RESPONSE_TAG,
            &topic_from_key(&notify_key),
        )
        .await;

    let (_id, auth) =
        decode_auth_message::<SubscriptionGetNotificationsResponseAuth>(msg, &notify_key)?;
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
    assert_eq!(auth.sub, account.to_did_pkh());

    let value = serde_json::to_value(&auth).unwrap();
    assert!(value.get("sub").is_some());
    assert!(value.get("nfs").is_some());
    assert!(value.get("mre").is_some());
    assert!(value.get("mur").is_some());
    assert!(value.get("notifications").is_none());
    assert!(value.get("has_more").is_none());
    assert!(value.get("has_more_unread").is_none());

    Ok(auth.result)
}

async fn publish_mark_notifications_as_read_request(
    relay_client: &mut RelayClient,
    account: &AccountId,
    client_id: &DecodedClientId,
    identity_key_details: &IdentityKeyDetails,
    sym_key: [u8; 32],
    app: DidWeb,
    params: MarkNotificationsAsReadParams,
) {
    publish_jwt_message(
        relay_client,
        client_id,
        identity_key_details,
        &TopicEncrptionScheme::Symetric(sym_key),
        NOTIFY_MARK_NOTIFICATIONS_AS_READ_TAG,
        NOTIFY_MARK_NOTIFICATIONS_AS_READ_TTL,
        NOTIFY_MARK_NOTIFICATIONS_AS_READ_ACT,
        None,
        |shared_claims| {
            serde_json::to_value(JsonRpcRequest::new(
                NOTIFY_MARK_NOTIFICATIONS_AS_READ_METHOD,
                AuthMessage {
                    auth: encode_auth(
                        &SubscriptionMarkNotificationsAsReadRequestAuth {
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
async fn helper_mark_notifications_as_read(
    relay_client: &mut RelayClient,
    account: &AccountId,
    identity_key_details: &IdentityKeyDetails,
    app: &DidWeb,
    app_client_id: &DecodedClientId,
    notify_key: [u8; 32],
    params: MarkNotificationsAsReadParams,
) -> Result<(), JsonRpcResponseError> {
    publish_mark_notifications_as_read_request(
        relay_client,
        account,
        app_client_id,
        identity_key_details,
        notify_key,
        app.clone(),
        params,
    )
    .await;

    let msg = relay_client
        .accept_message(
            NOTIFY_MARK_NOTIFICATIONS_AS_READ_RESPONSE_TAG,
            &topic_from_key(&notify_key),
        )
        .await;

    let (_id, auth) =
        decode_auth_message::<SubscriptionMarkNotificationsAsReadResponseAuth>(msg, &notify_key)?;
    assert_eq!(
        auth.shared_claims.act,
        NOTIFY_MARK_NOTIFICATIONS_AS_READ_RESPONSE_ACT
    );
    assert_eq!(auth.shared_claims.iss, app_client_id.to_did_key());
    assert_eq!(
        auth.shared_claims.aud,
        identity_key_details.client_id.to_did_key()
    );
    assert_eq!(&auth.app, app);
    assert_eq!(auth.sub, account.to_did_pkh());

    Ok(())
}

async fn subscribe_topic(
    project_id: &ProjectId,
    app_domain: DidWeb,
    notify_server_url: &Url,
) -> (x25519_dalek::PublicKey, VerifyingKey, DecodedClientId) {
    utils::http_api::subscribe_topic(project_id, Uuid::new_v4(), app_domain, notify_server_url)
        .await
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
            CacaoAuth::Statement(STATEMENT_THIS_DOMAIN.to_owned()),
            identity_public_key.clone(),
            identity_key_details.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
    )
    .await;

    let vars = get_vars();
    let mut relay_client = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    let (key_agreement, _authentication, client_id) =
        subscribe_topic(&project_id, app_domain.clone(), &notify_server.url).await;

    let (subs, watch_topic_key, notify_server_client_id) = watch_subscriptions(
        &mut relay_client,
        notify_server.url.clone(),
        &identity_key_details,
        Some(app_domain.clone()),
        &account,
    )
    .await
    .unwrap();
    assert!(subs.is_empty());

    // Subscribe with 1 type
    let notification_types = HashSet::from([Uuid::new_v4()]);
    let mut relay_client2 = relay_client.clone();
    subscribe(
        &mut relay_client,
        &account,
        &identity_key_details,
        key_agreement,
        &client_id,
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
    assert_eq!(sub.scope, notification_types);

    let notify_key = decode_key(&sub.sym_key).unwrap();

    relay_client.subscribe(topic_from_key(&notify_key)).await;

    // Update to 0 types
    let notification_types = HashSet::from([]);
    let mut relay_client2 = relay_client.clone();
    update(
        &mut relay_client,
        &account,
        &identity_key_details,
        &app_domain,
        &client_id,
        notify_key,
        &notification_types,
    )
    .await;
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
    assert_eq!(subs[0].scope, notification_types);

    // Update to 2 types
    let notification_types = HashSet::from([Uuid::new_v4(), Uuid::new_v4()]);
    let mut relay_client2 = relay_client.clone();
    update(
        &mut relay_client,
        &account,
        &identity_key_details,
        &app_domain,
        &client_id,
        notify_key,
        &notification_types,
    )
    .await;
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
    assert_eq!(subs[0].scope, notification_types);
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
            CacaoAuth::Statement(STATEMENT_THIS_DOMAIN.to_owned()),
            identity_public_key.clone(),
            identity_key_details.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
    )
    .await;

    let vars = get_vars();
    let mut relay_client = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    let (key_agreement, authentication, client_id) =
        subscribe_topic(&project_id, app_domain.clone(), &notify_server.url).await;

    let (subs, watch_topic_key, notify_server_client_id) = watch_subscriptions(
        &mut relay_client,
        notify_server.url.clone(),
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
        &client_id,
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
    assert_eq!(sub.scope, notification_types);

    let notify_key = decode_key(&sub.sym_key).unwrap();

    relay_client.subscribe(topic_from_key(&notify_key)).await;

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
        &mut relay_client,
        &identity_key_details,
        &account,
        &authentication,
        &client_id,
        app_domain.clone(),
        notify_key,
    )
    .await;

    assert_eq!(claims.msg.r#type, notification.r#type);
    assert_eq!(claims.msg.title, notification.title);
    assert_eq!(claims.msg.body, notification.body);
    assert_eq!(claims.msg.icon, "icon");
    assert_ne!(claims.msg.url, "");

    let mut relay_client2 = relay_client.clone();
    delete(
        &mut relay_client,
        &identity_key_details,
        &app_domain,
        &client_id,
        &account,
        notify_key,
    )
    .await;

    let sbs = accept_watch_subscriptions_changed(
        &mut relay_client2,
        &notify_server_client_id,
        &identity_key_details,
        &account,
        watch_topic_key,
    )
    .await
    .unwrap();
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
    .json::<notify_server::services::public_http_server::handlers::notify_v0::ResponseBody>()
    .await
    .unwrap();

    assert_eq!(resp.not_found.len(), 1);
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
            CacaoAuth::Statement(STATEMENT_ALL_DOMAINS.to_owned()),
            identity_public_key.clone(),
            identity_key_details.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
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
    let mut relay_client = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    let (subs, watch_topic_key, notify_server_client_id) = watch_subscriptions(
        &mut relay_client,
        notify_server.url.clone(),
        &identity_key_details,
        None,
        &account,
    )
    .await
    .unwrap();
    assert!(subs.is_empty());

    let notification_type1 = Uuid::new_v4();
    let notification_types1 = HashSet::from([notification_type1, Uuid::new_v4()]);
    let mut relay_client2 = relay_client.clone();
    subscribe(
        &mut relay_client,
        &account,
        &identity_key_details,
        key_agreement1,
        &client_id1,
        app_domain1.clone(),
        notification_types1.clone(),
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
    let mut relay_client2 = relay_client.clone();
    subscribe(
        &mut relay_client,
        &account,
        &identity_key_details,
        key_agreement2,
        &client_id2,
        app_domain2.clone(),
        notification_types2.clone(),
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
            CacaoAuth::Statement(STATEMENT_THIS_DOMAIN.to_owned()),
            identity_public_key.clone(),
            identity_key_details.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
    )
    .await;

    let project_id2 = ProjectId::generate();
    let app_domain2 = DidWeb::from_domain(format!("{project_id2}.example.com"));
    let (key_agreement2, _authentication2, client_id2) =
        subscribe_topic(&project_id2, app_domain2.clone(), &notify_server.url).await;

    let vars = get_vars();
    let mut relay_client = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    let (subs, watch_topic_key, notify_server_client_id) = watch_subscriptions(
        &mut relay_client,
        notify_server.url.clone(),
        &identity_key_details,
        Some(app_domain1.clone()),
        &account,
    )
    .await
    .unwrap();
    assert!(subs.is_empty());

    let notification_type1 = Uuid::new_v4();
    let notification_types1 = HashSet::from([notification_type1, Uuid::new_v4()]);
    let mut relay_client2 = relay_client.clone();
    subscribe(
        &mut relay_client,
        &account,
        &identity_key_details,
        key_agreement1,
        &client_id1,
        app_domain1.clone(),
        notification_types1.clone(),
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
    assert_eq!(sub.app_domain, app_domain1.domain());

    let notification_type2 = Uuid::new_v4();
    let notification_types2 = HashSet::from([notification_type2, Uuid::new_v4()]);
    let mut relay_client2 = relay_client.clone();
    let result = subscribe(
        &mut relay_client,
        &account,
        &identity_key_details,
        key_agreement2,
        &client_id2,
        app_domain2.clone(),
        notification_types2.clone(),
    )
    .await;
    assert!(result.is_err());
    let result = tokio::time::timeout(
        RELAY_MESSAGE_DELIVERY_TIMEOUT / 2,
        accept_watch_subscriptions_changed(
            &mut relay_client2,
            &notify_server_client_id,
            &identity_key_details,
            &account,
            watch_topic_key,
        ),
    )
    .await;
    assert!(result.is_err());
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn all_apps_works_recaps(notify_server: &NotifyServerContext) {
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
            CacaoAuth::AllApps,
            identity_public_key.clone(),
            identity_key_details.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
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
    let mut relay_client = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    let (subs, watch_topic_key, notify_server_client_id) = watch_subscriptions(
        &mut relay_client,
        notify_server.url.clone(),
        &identity_key_details,
        None,
        &account,
    )
    .await
    .unwrap();
    assert!(subs.is_empty());

    let notification_type1 = Uuid::new_v4();
    let notification_types1 = HashSet::from([notification_type1, Uuid::new_v4()]);
    let mut relay_client2 = relay_client.clone();
    subscribe(
        &mut relay_client,
        &account,
        &identity_key_details,
        key_agreement1,
        &client_id1,
        app_domain1.clone(),
        notification_types1.clone(),
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
    let mut relay_client2 = relay_client.clone();
    subscribe(
        &mut relay_client,
        &account,
        &identity_key_details,
        key_agreement2,
        &client_id2,
        app_domain2.clone(),
        notification_types2.clone(),
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
async fn this_app_only_recaps(notify_server: &NotifyServerContext) {
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
            CacaoAuth::ThisApp,
            identity_public_key.clone(),
            identity_key_details.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
    )
    .await;

    let project_id2 = ProjectId::generate();
    let app_domain2 = DidWeb::from_domain(format!("{project_id2}.example.com"));
    let (key_agreement2, _authentication2, client_id2) =
        subscribe_topic(&project_id2, app_domain2.clone(), &notify_server.url).await;

    let vars = get_vars();
    let mut relay_client = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    let (subs, watch_topic_key, notify_server_client_id) = watch_subscriptions(
        &mut relay_client,
        notify_server.url.clone(),
        &identity_key_details,
        Some(app_domain1.clone()),
        &account,
    )
    .await
    .unwrap();
    assert!(subs.is_empty());

    let notification_type1 = Uuid::new_v4();
    let notification_types1 = HashSet::from([notification_type1, Uuid::new_v4()]);
    let mut relay_client2 = relay_client.clone();
    subscribe(
        &mut relay_client,
        &account,
        &identity_key_details,
        key_agreement1,
        &client_id1,
        app_domain1.clone(),
        notification_types1.clone(),
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
    assert_eq!(sub.app_domain, app_domain1.domain());

    let notification_type2 = Uuid::new_v4();
    let notification_types2 = HashSet::from([notification_type2, Uuid::new_v4()]);
    let mut relay_client2 = relay_client.clone();
    let result = subscribe(
        &mut relay_client,
        &account,
        &identity_key_details,
        key_agreement2,
        &client_id2,
        app_domain2.clone(),
        notification_types2.clone(),
    )
    .await;
    assert!(result.is_err());
    let result = tokio::time::timeout(
        RELAY_MESSAGE_DELIVERY_TIMEOUT / 2,
        accept_watch_subscriptions_changed(
            &mut relay_client2,
            &notify_server_client_id,
            &identity_key_details,
            &account,
            watch_topic_key,
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

    let vars = get_vars();
    let mut relay_client = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    let (_subs, _watch_topic_key, _notify_server_client_id) = watch_subscriptions(
        &mut relay_client,
        notify_server.url.clone(),
        &identity_key_details,
        Some(app_domain),
        &account,
    )
    .await
    .unwrap();

    unregister_identity_key(
        identity_key_details.keys_server_url,
        &account,
        &identity_key_details.signing_key,
        &identity_public_key,
    )
    .await;
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn works_with_staging_keys_server_recaps(notify_server: &NotifyServerContext) {
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
                    CacaoAuth::ThisApp,
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

    let vars = get_vars();
    let mut relay_client = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    let (_subs, _watch_topic_key, _notify_server_client_id) = watch_subscriptions(
        &mut relay_client,
        notify_server.url.clone(),
        &identity_key_details,
        Some(app_domain),
        &account,
    )
    .await
    .unwrap();

    unregister_identity_key(
        identity_key_details.keys_server_url,
        &account,
        &identity_key_details.signing_key,
        &identity_public_key,
    )
    .await;
}

async fn setup_project_and_watch(
    notify_server_url: Url,
) -> (
    RelayClient,
    AccountId,
    IdentityKeyDetails,
    ProjectId,
    DidWeb,
    DecodedClientId,
    PublicKey,
    VerifyingKey,
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
            CacaoAuth::Statement(STATEMENT_THIS_DOMAIN.to_owned()),
            identity_public_key.clone(),
            identity_key_details.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
    )
    .await;

    let vars = get_vars();
    let mut relay_client = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server_url.clone(),
    )
    .await;

    let (app_key_agreement_key, app_authentication_key, app_client_id) =
        subscribe_topic(&project_id, app_domain.clone(), &notify_server_url).await;

    let (subs, watch_topic_key, notify_server_client_id) = watch_subscriptions(
        &mut relay_client,
        notify_server_url,
        &identity_key_details,
        Some(app_domain.clone()),
        &account,
    )
    .await
    .unwrap();
    assert!(subs.is_empty());

    (
        relay_client,
        account,
        identity_key_details,
        project_id,
        app_domain,
        app_client_id,
        app_key_agreement_key,
        app_authentication_key,
        notify_server_client_id,
        watch_topic_key,
    )
}

#[allow(clippy::too_many_arguments)]
async fn subscribe_to_notifications(
    relay_client: &mut RelayClient,
    account: &AccountId,
    identity_key_details: &IdentityKeyDetails,
    app_domain: DidWeb,
    app_client_id: &DecodedClientId,
    app_key_agreement_key: PublicKey,
    notify_server_client_id: &DecodedClientId,
    watch_topic_key: [u8; 32],
    notification_types: HashSet<Uuid>,
) -> [u8; 32] {
    let mut relay_client2 = relay_client.clone();
    subscribe(
        relay_client,
        account,
        identity_key_details,
        app_key_agreement_key,
        app_client_id,
        app_domain,
        notification_types,
    )
    .await
    .unwrap();
    let subs = accept_watch_subscriptions_changed(
        &mut relay_client2,
        notify_server_client_id,
        identity_key_details,
        account,
        watch_topic_key,
    )
    .await
    .unwrap();
    assert_eq!(subs.len(), 1);
    let sub = &subs[0];

    let notify_key = decode_key(&sub.sym_key).unwrap();

    relay_client.subscribe(topic_from_key(&notify_key)).await;

    notify_key
}

async fn setup_subscription(
    notify_server_url: Url,
    notification_types: HashSet<Uuid>,
) -> (
    RelayClient,
    AccountId,
    IdentityKeyDetails,
    ProjectId,
    DidWeb,
    VerifyingKey,
    DecodedClientId,
    [u8; 32],
) {
    let (
        mut relay_client,
        account,
        identity_key_details,
        project_id,
        app_domain,
        app_client_id,
        app_key_agreement_key,
        app_authentication_key,
        notify_server_client_id,
        watch_topic_key,
    ) = setup_project_and_watch(notify_server_url).await;

    let notify_key = subscribe_to_notifications(
        &mut relay_client,
        &account,
        &identity_key_details,
        app_domain.clone(),
        &app_client_id,
        app_key_agreement_key,
        &notify_server_client_id,
        watch_topic_key,
        notification_types,
    )
    .await;

    (
        relay_client,
        account,
        identity_key_details,
        project_id,
        app_domain,
        app_authentication_key,
        app_client_id,
        notify_key,
    )
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn e2e_get_notifications_has_none(notify_server: &NotifyServerContext) {
    let (
        mut relay_client,
        account,
        identity_key_details,
        _project_id,
        app_domain,
        _app_authentication_key,
        app_client_id,
        notify_key,
    ) = setup_subscription(notify_server.url.clone(), HashSet::from([Uuid::new_v4()])).await;

    let result = get_notifications(
        &mut relay_client,
        &account,
        &identity_key_details,
        &app_domain,
        &app_client_id,
        notify_key,
        GetNotificationsParams {
            limit: 5,
            after: None,
            unread_first: false,
        },
    )
    .await
    .unwrap();
    assert!(result.notifications.is_empty());
    assert!(!result.has_more);

    let failed_result = get_notifications(
        &mut relay_client,
        &account,
        &identity_key_details,
        &app_domain,
        &app_client_id,
        notify_key,
        GetNotificationsParams {
            limit: 51, // larger than the maximum of 50
            after: None,
            unread_first: false,
        },
    )
    .await;
    assert!(failed_result.is_err());
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn e2e_get_notifications_has_one(notify_server: &NotifyServerContext) {
    let notification_type = Uuid::new_v4();
    let (
        mut relay_client,
        account,
        identity_key_details,
        project_id,
        app_domain,
        _app_authentication_key,
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

    let before_notification_sent = Utc::now() - chrono::Duration::seconds(1);
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
    let after_notification_sent = Utc::now() + chrono::Duration::seconds(1); // Postgres time could be slightly out-of-sync with this process it seems

    let result = get_notifications(
        &mut relay_client,
        &account,
        &identity_key_details,
        &app_domain,
        &app_client_id,
        notify_key,
        GetNotificationsParams {
            limit: 5,
            after: None,
            unread_first: false,
        },
    )
    .await
    .unwrap();
    assert_eq!(result.notifications.len(), 1);
    assert!(!result.has_more);

    let gotten_notification = &result.notifications[0];
    assert_eq!(notification.r#type, gotten_notification.r#type);
    assert_eq!(notification.title, gotten_notification.title);
    assert_eq!(notification.body, gotten_notification.body);

    assert!(gotten_notification.sent_at >= before_notification_sent.timestamp_millis());
    assert!(gotten_notification.sent_at <= after_notification_sent.timestamp_millis());
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
    let SubscribeResponse { id: subscriber, .. } = upsert_subscriber(
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
            unread_first: false,
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
    let SubscribeResponse { id: subscriber, .. } = upsert_subscriber(
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
            unread_first: false,
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
    let SubscribeResponse { id: subscriber, .. } = upsert_subscriber(
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
            unread_first: false,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.notifications.len(), notification_titles.len());
    assert!(!result.has_more);

    for (n1, n2) in result.notifications.iter().tuple_windows() {
        assert!(n1.sent_at >= n2.sent_at);
    }

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
    let SubscribeResponse { id: subscriber, .. } = upsert_subscriber(
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
        GetNotificationsParams {
            limit,
            after: None,
            unread_first: false,
        },
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
    let SubscribeResponse { id: subscriber, .. } = upsert_subscriber(
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
        GetNotificationsParams {
            limit,
            after: None,
            unread_first: false,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(first_page.notifications.len(), limit);
    assert!(first_page.has_more);

    let last_id = first_page.notifications.last().unwrap().id;
    for notification in &first_page.notifications {
        gotten_ids.insert(notification.id);
        gotten_titles.insert(notification.title.clone());
    }

    let second_page = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams {
            limit,
            after: Some(last_id),
            unread_first: false,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert!(second_page.notifications.len() < limit);
    assert_eq!(second_page.notifications.len(), 1);
    assert!(!second_page.has_more);

    assert!(
        first_page.notifications.last().unwrap().sent_at >= second_page.notifications[0].sent_at
    );

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
    let SubscribeResponse { id: subscriber, .. } = upsert_subscriber(
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
        GetNotificationsParams {
            limit,
            after: None,
            unread_first: false,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(first_page.notifications.len(), limit);
    assert!(first_page.has_more);

    assert_eq!(&first_page.notifications[0].title, notification_titles[6]);
    assert_eq!(&first_page.notifications[1].title, notification_titles[5]);
    assert_eq!(&first_page.notifications[2].title, notification_titles[4]);
    assert_eq!(&first_page.notifications[3].title, notification_titles[3]);
    assert_eq!(&first_page.notifications[4].title, notification_titles[2]);

    let second_page = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams {
            limit,
            after: Some(first_page.notifications.last().unwrap().id),
            unread_first: false,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert!(second_page.notifications.len() < limit);
    assert_eq!(second_page.notifications.len(), 2);
    assert!(!second_page.has_more);

    assert_eq!(&second_page.notifications[0].title, notification_titles[1]);
    assert_eq!(&second_page.notifications[1].title, notification_titles[0]);

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
    let SubscribeResponse { id: subscriber, .. } = upsert_subscriber(
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
        GetNotificationsParams {
            limit,
            after: None,
            unread_first: false,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(first_page.notifications.len(), limit);
    assert!(first_page.has_more);

    assert_eq!(&first_page.notifications[0].title, notification_titles[6]);
    assert_eq!(&first_page.notifications[1].title, notification_titles[5]);
    assert_eq!(&first_page.notifications[2].title, notification_titles[4]);
    assert_eq!(&first_page.notifications[3].title, notification_titles[3]);
    assert_eq!(&first_page.notifications[4].title, notification_titles[2]);

    let second_page = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams {
            limit,
            after: Some(first_page.notifications.last().unwrap().id),
            unread_first: false,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert!(second_page.notifications.len() < limit);
    assert_eq!(second_page.notifications.len(), 2);
    assert!(!second_page.has_more);

    assert_eq!(&second_page.notifications[0].title, notification_titles[1]);
    assert_eq!(&second_page.notifications[1].title, notification_titles[0]);
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
    let SubscribeResponse { id: subscriber, .. } = upsert_subscriber(
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
        GetNotificationsParams {
            limit,
            after: None,
            unread_first: false,
        },
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
            unread_first: false,
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

#[tokio::test]
async fn get_no_welcome_notification() {
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

    let result = get_welcome_notification(project.id, &postgres, None)
        .await
        .unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn set_a_welcome_notification() {
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

    let welcome_notification = WelcomeNotification {
        enabled: true,
        r#type: Uuid::new_v4(),
        title: "title".to_owned(),
        body: "body".to_owned(),
        url: None,
    };

    set_welcome_notification(project.id, welcome_notification.clone(), &postgres, None)
        .await
        .unwrap();

    let got_notification = get_welcome_notification(project.id, &postgres, None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(welcome_notification.enabled, got_notification.enabled);
    assert_eq!(welcome_notification.r#type, got_notification.r#type);
    assert_eq!(welcome_notification.title, got_notification.title);
    assert_eq!(welcome_notification.body, got_notification.body);
    assert_eq!(welcome_notification.url, got_notification.url);
}

#[tokio::test]
async fn set_a_welcome_notification_for_different_project() {
    let (postgres, _) = get_postgres().await;

    let topic1 = Topic::generate();
    let project_id1 = ProjectId::generate();
    let subscribe_key1 = generate_subscribe_key();
    let authentication_key1 = generate_authentication_key();
    let app_domain1 = generate_app_domain();
    upsert_project(
        project_id1.clone(),
        &app_domain1,
        topic1,
        &authentication_key1,
        &subscribe_key1,
        &postgres,
        None,
    )
    .await
    .unwrap();
    let project1 = get_project_by_project_id(project_id1.clone(), &postgres, None)
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

    let welcome_notification = WelcomeNotification {
        enabled: true,
        r#type: Uuid::new_v4(),
        title: "title".to_owned(),
        body: "body".to_owned(),
        url: None,
    };

    set_welcome_notification(project1.id, welcome_notification, &postgres, None)
        .await
        .unwrap();

    let got_notification = get_welcome_notification(project2.id, &postgres, None)
        .await
        .unwrap();
    assert!(got_notification.is_none());
}

#[tokio::test]
async fn update_welcome_notification() {
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

    {
        let welcome_notification = WelcomeNotification {
            enabled: true,
            r#type: Uuid::new_v4(),
            title: "title".to_owned(),
            body: "body".to_owned(),
            url: None,
        };
        set_welcome_notification(project.id, welcome_notification.clone(), &postgres, None)
            .await
            .unwrap();
        let got_notification = get_welcome_notification(project.id, &postgres, None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(welcome_notification.enabled, got_notification.enabled);
        assert_eq!(welcome_notification.r#type, got_notification.r#type);
        assert_eq!(welcome_notification.title, got_notification.title);
        assert_eq!(welcome_notification.body, got_notification.body);
        assert_eq!(welcome_notification.url, got_notification.url);
    }

    {
        let welcome_notification = WelcomeNotification {
            enabled: false,
            r#type: Uuid::new_v4(),
            title: "title2".to_owned(),
            body: "body2".to_owned(),
            url: Some("url".to_owned()),
        };
        set_welcome_notification(project.id, welcome_notification.clone(), &postgres, None)
            .await
            .unwrap();
        let got_notification = get_welcome_notification(project.id, &postgres, None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(welcome_notification.enabled, got_notification.enabled);
        assert_eq!(welcome_notification.r#type, got_notification.r#type);
        assert_eq!(welcome_notification.title, got_notification.title);
        assert_eq!(welcome_notification.body, got_notification.body);
        assert_eq!(welcome_notification.url, got_notification.url);
    }
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn http_get_no_welcome_notification(notify_server: &NotifyServerContext) {
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
        &notify_server.postgres,
        None,
    )
    .await
    .unwrap();

    let got_notification = assert_successful_response(
        reqwest::Client::new()
            .get(
                notify_server
                    .url
                    .join(&format!("/v0/{project_id}/welcome-notification"))
                    .unwrap(),
            )
            .bearer_auth(Uuid::new_v4())
            .send()
            .await
            .unwrap(),
    )
    .await
    .json::<Option<WelcomeNotification>>()
    .await
    .unwrap();
    assert!(got_notification.is_none());
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn http_get_a_welcome_notification(notify_server: &NotifyServerContext) {
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
        &notify_server.postgres,
        None,
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &notify_server.postgres, None)
        .await
        .unwrap();

    let welcome_notification = WelcomeNotification {
        enabled: true,
        r#type: Uuid::new_v4(),
        title: "title".to_owned(),
        body: "body".to_owned(),
        url: None,
    };

    set_welcome_notification(
        project.id,
        welcome_notification.clone(),
        &notify_server.postgres,
        None,
    )
    .await
    .unwrap();

    let got_notification = assert_successful_response(
        reqwest::Client::new()
            .get(
                notify_server
                    .url
                    .join(&format!("/v0/{project_id}/welcome-notification"))
                    .unwrap(),
            )
            .bearer_auth(Uuid::new_v4())
            .send()
            .await
            .unwrap(),
    )
    .await
    .json::<Option<WelcomeNotification>>()
    .await
    .unwrap()
    .unwrap();
    assert_eq!(welcome_notification.enabled, got_notification.enabled);
    assert_eq!(welcome_notification.r#type, got_notification.r#type);
    assert_eq!(welcome_notification.title, got_notification.title);
    assert_eq!(welcome_notification.body, got_notification.body);
    assert_eq!(welcome_notification.url, got_notification.url);
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn http_set_a_welcome_notification(notify_server: &NotifyServerContext) {
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
        &notify_server.postgres,
        None,
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &notify_server.postgres, None)
        .await
        .unwrap();

    let welcome_notification = WelcomeNotification {
        enabled: true,
        r#type: Uuid::new_v4(),
        title: "title".to_owned(),
        body: "body".to_owned(),
        url: None,
    };

    assert_successful_response(
        reqwest::Client::new()
            .post(
                notify_server
                    .url
                    .join(&format!("/v0/{project_id}/welcome-notification"))
                    .unwrap(),
            )
            .bearer_auth(Uuid::new_v4())
            .json(&welcome_notification)
            .send()
            .await
            .unwrap(),
    )
    .await;

    let got_notification = get_welcome_notification(project.id, &notify_server.postgres, None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(welcome_notification.enabled, got_notification.enabled);
    assert_eq!(welcome_notification.r#type, got_notification.r#type);
    assert_eq!(welcome_notification.title, got_notification.title);
    assert_eq!(welcome_notification.body, got_notification.body);
    assert_eq!(welcome_notification.url, got_notification.url);
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn e2e_set_a_welcome_notification(notify_server: &NotifyServerContext) {
    let project_id = ProjectId::generate();
    let app_domain = DidWeb::from_domain(format!("{project_id}.walletconnect.com"));

    let (_app_key_agreement_key, _app_authentication_key, _app_client_id) =
        subscribe_topic(&project_id, app_domain.clone(), &notify_server.url).await;

    let welcome_notification = WelcomeNotification {
        enabled: true,
        r#type: Uuid::new_v4(),
        title: "title".to_owned(),
        body: "body".to_owned(),
        url: None,
    };

    assert_successful_response(
        reqwest::Client::new()
            .post(
                notify_server
                    .url
                    .join(&format!("/v0/{project_id}/welcome-notification"))
                    .unwrap(),
            )
            .bearer_auth(Uuid::new_v4())
            .json(&welcome_notification)
            .send()
            .await
            .unwrap(),
    )
    .await;

    let got_notification = assert_successful_response(
        reqwest::Client::new()
            .get(
                notify_server
                    .url
                    .join(&format!("/v0/{project_id}/welcome-notification"))
                    .unwrap(),
            )
            .bearer_auth(Uuid::new_v4())
            .send()
            .await
            .unwrap(),
    )
    .await
    .json::<Option<WelcomeNotification>>()
    .await
    .unwrap()
    .unwrap();
    assert_eq!(welcome_notification.enabled, got_notification.enabled);
    assert_eq!(welcome_notification.r#type, got_notification.r#type);
    assert_eq!(welcome_notification.title, got_notification.title);
    assert_eq!(welcome_notification.body, got_notification.body);
    assert_eq!(welcome_notification.url, got_notification.url);
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn e2e_send_welcome_notification(notify_server: &NotifyServerContext) {
    let (
        mut relay_client,
        account,
        identity_key_details,
        project_id,
        app_domain,
        app_client_id,
        app_key_agreement_key,
        app_authentication_key,
        notify_server_client_id,
        watch_topic_key,
    ) = setup_project_and_watch(notify_server.url.clone()).await;

    let notification_type = Uuid::new_v4();
    let welcome_notification = WelcomeNotification {
        enabled: true,
        r#type: notification_type,
        title: "title".to_owned(),
        body: "body".to_owned(),
        url: Some("url".to_owned()),
    };

    assert_successful_response(
        reqwest::Client::new()
            .post(
                notify_server
                    .url
                    .join(&format!("/v0/{project_id}/welcome-notification"))
                    .unwrap(),
            )
            .bearer_auth(Uuid::new_v4())
            .json(&welcome_notification)
            .send()
            .await
            .unwrap(),
    )
    .await;

    let mut relay_client2 = relay_client.clone();

    let notify_key = subscribe_to_notifications(
        &mut relay_client,
        &account,
        &identity_key_details,
        app_domain.clone(),
        &app_client_id,
        app_key_agreement_key,
        &notify_server_client_id,
        watch_topic_key,
        HashSet::from([notification_type]),
    )
    .await;

    let NotifyMessage {
        msg: notify_message,
        ..
    } = accept_and_respond_to_notify_message(
        &mut relay_client2,
        &identity_key_details,
        &account,
        &app_authentication_key,
        &app_client_id,
        app_domain.clone(),
        notify_key,
    )
    .await;
    assert_eq!(welcome_notification.r#type, notify_message.r#type);
    assert_eq!(welcome_notification.title, notify_message.title);
    assert_eq!(welcome_notification.body, notify_message.body);
    assert!(welcome_notification.url.is_some());
    assert_ne!(welcome_notification.url.as_ref(), Some(&notify_message.url));

    let result = get_notifications(
        &mut relay_client,
        &account,
        &identity_key_details,
        &app_domain,
        &app_client_id,
        notify_key,
        GetNotificationsParams {
            limit: 5,
            after: None,
            unread_first: false,
        },
    )
    .await
    .unwrap();
    assert_eq!(result.notifications.len(), 1);
    assert!(!result.has_more);

    let gotten_notification = &result.notifications[0];
    assert_eq!(welcome_notification.r#type, gotten_notification.r#type);
    assert_eq!(welcome_notification.title, gotten_notification.title);
    assert_eq!(welcome_notification.body, gotten_notification.body);
    assert!(gotten_notification.url.is_some());
    assert_ne!(welcome_notification.url, gotten_notification.url);
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn e2e_send_single_welcome_notification(notify_server: &NotifyServerContext) {
    let (
        mut relay_client,
        account,
        identity_key_details,
        project_id,
        app_domain,
        app_client_id,
        app_key_agreement_key,
        _app_authentication_key,
        notify_server_client_id,
        watch_topic_key,
    ) = setup_project_and_watch(notify_server.url.clone()).await;
    let project = get_project_by_project_id(project_id.clone(), &notify_server.postgres, None)
        .await
        .unwrap();

    let notification_type = Uuid::new_v4();
    set_welcome_notification(
        project.id,
        WelcomeNotification {
            enabled: true,
            r#type: notification_type,
            title: "title".to_owned(),
            body: "body".to_owned(),
            url: None,
        },
        &notify_server.postgres,
        None,
    )
    .await
    .unwrap();

    let _notify_key = subscribe_to_notifications(
        &mut relay_client,
        &account,
        &identity_key_details,
        app_domain.clone(),
        &app_client_id,
        app_key_agreement_key,
        &notify_server_client_id,
        watch_topic_key,
        HashSet::from([notification_type]),
    )
    .await;

    let notify_key = subscribe_to_notifications(
        &mut relay_client,
        &account,
        &identity_key_details,
        app_domain.clone(),
        &app_client_id,
        app_key_agreement_key,
        &notify_server_client_id,
        watch_topic_key,
        HashSet::from([notification_type]),
    )
    .await;

    let result = get_notifications(
        &mut relay_client,
        &account,
        &identity_key_details,
        &app_domain,
        &app_client_id,
        notify_key,
        GetNotificationsParams {
            limit: 5,
            after: None,
            unread_first: false,
        },
    )
    .await
    .unwrap();
    assert_eq!(result.notifications.len(), 1);
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn subscribe_idempotent_keeps_symkey(notify_server: &NotifyServerContext) {
    let (
        mut relay_client,
        account,
        identity_key_details,
        _project_id,
        app_domain,
        app_client_id,
        app_key_agreement_key,
        _app_authentication_key,
        notify_server_client_id,
        watch_topic_key,
    ) = setup_project_and_watch(notify_server.url.clone()).await;

    let notify_key1 = subscribe_to_notifications(
        &mut relay_client,
        &account,
        &identity_key_details,
        app_domain.clone(),
        &app_client_id,
        app_key_agreement_key,
        &notify_server_client_id,
        watch_topic_key,
        HashSet::from([Uuid::new_v4()]),
    )
    .await;

    let notify_key2 = subscribe_to_notifications(
        &mut relay_client,
        &account,
        &identity_key_details,
        app_domain.clone(),
        &app_client_id,
        app_key_agreement_key,
        &notify_server_client_id,
        watch_topic_key,
        HashSet::from([Uuid::new_v4()]),
    )
    .await;

    assert_eq!(notify_key1, notify_key2);
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn subscribe_idempotent_updates_notification_types(notify_server: &NotifyServerContext) {
    let (
        mut relay_client,
        account,
        identity_key_details,
        _project_id,
        app_domain,
        app_client_id,
        app_key_agreement_key,
        _app_authentication_key,
        _notify_server_client_id,
        _watch_topic_key,
    ) = setup_project_and_watch(notify_server.url.clone()).await;

    let notification_types = HashSet::from([Uuid::new_v4()]);
    let subs = subscribe_v1(
        &mut relay_client,
        &account,
        &identity_key_details,
        app_key_agreement_key,
        &app_client_id,
        app_domain.clone(),
        notification_types.clone(),
    )
    .await;
    assert_eq!(subs[0].scope, notification_types);

    let notification_types = HashSet::from([Uuid::new_v4()]);
    let subs = subscribe_v1(
        &mut relay_client,
        &account,
        &identity_key_details,
        app_key_agreement_key,
        &app_client_id,
        app_domain,
        notification_types.clone(),
    )
    .await;
    assert_eq!(subs[0].scope, notification_types);
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn e2e_doesnt_send_welcome_notification(notify_server: &NotifyServerContext) {
    let (
        mut relay_client,
        account,
        identity_key_details,
        project_id,
        app_domain,
        app_client_id,
        app_key_agreement_key,
        app_authentication_key,
        notify_server_client_id,
        watch_topic_key,
    ) = setup_project_and_watch(notify_server.url.clone()).await;

    let notification_type = Uuid::new_v4();
    let welcome_notification = WelcomeNotification {
        enabled: false,
        r#type: notification_type,
        title: "title".to_owned(),
        body: "body".to_owned(),
        url: Some("url".to_owned()),
    };

    assert_successful_response(
        reqwest::Client::new()
            .post(
                notify_server
                    .url
                    .join(&format!("/v0/{project_id}/welcome-notification"))
                    .unwrap(),
            )
            .bearer_auth(Uuid::new_v4())
            .json(&welcome_notification)
            .send()
            .await
            .unwrap(),
    )
    .await;

    let mut relay_client2 = relay_client.clone();

    let notify_key = subscribe_to_notifications(
        &mut relay_client,
        &account,
        &identity_key_details,
        app_domain.clone(),
        &app_client_id,
        app_key_agreement_key,
        &notify_server_client_id,
        watch_topic_key,
        HashSet::from([notification_type]),
    )
    .await;

    let result = tokio::time::timeout(
        RELAY_MESSAGE_DELIVERY_TIMEOUT / 2,
        accept_and_respond_to_notify_message(
            &mut relay_client2,
            &identity_key_details,
            &account,
            &app_authentication_key,
            &app_client_id,
            app_domain.clone(),
            notify_key,
        ),
    )
    .await;
    assert!(result.is_err());

    let result = get_notifications(
        &mut relay_client,
        &account,
        &identity_key_details,
        &app_domain,
        &app_client_id,
        notify_key,
        GetNotificationsParams {
            limit: 5,
            after: None,
            unread_first: false,
        },
    )
    .await
    .unwrap();
    assert!(result.notifications.is_empty());
    assert!(!result.has_more);
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn delete_and_resubscribe(notify_server: &NotifyServerContext) {
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
            CacaoAuth::Statement(STATEMENT_THIS_DOMAIN.to_owned()),
            identity_public_key.clone(),
            identity_key_details.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
    )
    .await;

    let vars = get_vars();
    let mut relay_client = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    let (key_agreement, authentication, client_id) =
        subscribe_topic(&project_id, app_domain.clone(), &notify_server.url).await;

    let (subs, watch_topic_key, notify_server_client_id) = watch_subscriptions(
        &mut relay_client,
        notify_server.url.clone(),
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
        &client_id,
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
    assert_eq!(sub.scope, notification_types);

    let notify_key = decode_key(&sub.sym_key).unwrap();

    relay_client.subscribe(topic_from_key(&notify_key)).await;

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
        &mut relay_client,
        &identity_key_details,
        &account,
        &authentication,
        &client_id,
        app_domain.clone(),
        notify_key,
    )
    .await;

    assert_eq!(claims.msg.r#type, notification.r#type);
    assert_eq!(claims.msg.title, notification.title);
    assert_eq!(claims.msg.body, notification.body);
    assert_eq!(claims.msg.icon, "icon");
    assert_ne!(claims.msg.url, "");

    let mut relay_client2 = relay_client.clone();
    delete(
        &mut relay_client,
        &identity_key_details,
        &app_domain,
        &client_id,
        &account,
        notify_key,
    )
    .await;

    let sbs = accept_watch_subscriptions_changed(
        &mut relay_client2,
        &notify_server_client_id,
        &identity_key_details,
        &account,
        watch_topic_key,
    )
    .await
    .unwrap();
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
    .json::<notify_server::services::public_http_server::handlers::notify_v0::ResponseBody>()
    .await
    .unwrap();

    assert_eq!(resp.not_found.len(), 1);

    let notification_type = Uuid::new_v4();
    let notification_types = HashSet::from([notification_type]);
    let mut relay_client2 = relay_client.clone();
    subscribe(
        &mut relay_client,
        &account,
        &identity_key_details,
        key_agreement,
        &client_id,
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
    assert_eq!(sub.scope, notification_types);

    let notify_key = decode_key(&sub.sym_key).unwrap();

    relay_client.subscribe(topic_from_key(&notify_key)).await;

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
        &mut relay_client,
        &identity_key_details,
        &account,
        &authentication,
        &client_id,
        app_domain.clone(),
        notify_key,
    )
    .await;

    assert_eq!(claims.msg.r#type, notification.r#type);
    assert_eq!(claims.msg.title, notification.title);
    assert_eq!(claims.msg.body, notification.body);
    assert_eq!(claims.msg.icon, "icon");
    assert_ne!(claims.msg.url, "");
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn watch_subscriptions_multiple_clients_mjv_v0(notify_server: &NotifyServerContext) {
    let (account_signing_key, account) = generate_account();

    let keys_server = MockServer::start().await;
    let keys_server_url = keys_server.uri().parse::<Url>().unwrap();

    let project_id = ProjectId::generate();
    let app_domain = DidWeb::from_domain(format!("{project_id}.walletconnect.com"));

    let (key_agreement, _authentication, client_id) =
        subscribe_topic(&project_id, app_domain.clone(), &notify_server.url).await;

    let (identity_signing_key1, identity_public_key1) = generate_identity_key();
    let identity_key_details1 = IdentityKeyDetails {
        keys_server_url: keys_server_url.clone(),
        signing_key: identity_signing_key1,
        client_id: identity_public_key1.clone(),
    };
    register_mocked_identity_key(
        &keys_server,
        identity_public_key1.clone(),
        sign_cacao(
            &app_domain,
            &account,
            CacaoAuth::Statement(STATEMENT_THIS_DOMAIN.to_owned()),
            identity_public_key1.clone(),
            identity_key_details1.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
    )
    .await;

    let vars = get_vars();
    let mut relay_client1 = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    let (identity_signing_key2, identity_public_key2) = generate_identity_key();
    let identity_key_details2 = IdentityKeyDetails {
        keys_server_url,
        signing_key: identity_signing_key2,
        client_id: identity_public_key2.clone(),
    };
    register_mocked_identity_key(
        &keys_server,
        identity_public_key2.clone(),
        sign_cacao(
            &app_domain,
            &account,
            CacaoAuth::Statement(STATEMENT_THIS_DOMAIN.to_owned()),
            identity_public_key2.clone(),
            identity_key_details2.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
    )
    .await;

    let vars = get_vars();
    let mut relay_client2 = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    let (subs1, watch_topic_key1, notify_server_client_id) = watch_subscriptions(
        &mut relay_client1,
        notify_server.url.clone(),
        &identity_key_details1,
        Some(app_domain.clone()),
        &account,
    )
    .await
    .unwrap();
    assert!(subs1.is_empty());

    let (subs2, watch_topic_key2, notify_server_client_id2) = watch_subscriptions(
        &mut relay_client2,
        notify_server.url.clone(),
        &identity_key_details2,
        Some(app_domain.clone()),
        &account,
    )
    .await
    .unwrap();
    assert!(subs2.is_empty());
    assert_eq!(notify_server_client_id2, notify_server_client_id);

    let notification_type = Uuid::new_v4();
    let notification_types = HashSet::from([notification_type]);
    let mut relay_client1_2 = relay_client1.clone();
    subscribe(
        &mut relay_client1,
        &account,
        &identity_key_details1,
        key_agreement,
        &client_id,
        app_domain.clone(),
        notification_types.clone(),
    )
    .await
    .unwrap();

    let subs1 = accept_watch_subscriptions_changed(
        &mut relay_client1_2,
        &notify_server_client_id,
        &identity_key_details1,
        &account,
        watch_topic_key1,
    )
    .await
    .unwrap();
    assert_eq!(subs1.len(), 1);
    let sub1 = &subs1[0];
    assert_eq!(sub1.scope, notification_types);

    let subs2 = accept_watch_subscriptions_changed(
        &mut relay_client2,
        &notify_server_client_id,
        &identity_key_details2,
        &account,
        watch_topic_key2,
    )
    .await
    .unwrap();
    assert_eq!(subs2.len(), 1);
    let sub2 = &subs2[0];
    assert_eq!(sub2.scope, notification_types);

    assert_eq!(sub1.sym_key, sub2.sym_key);
    let notify_key = decode_key(&sub2.sym_key).unwrap();

    relay_client1.subscribe(topic_from_key(&notify_key)).await;

    // Update to 2 types
    let notification_types = HashSet::from([Uuid::new_v4(), Uuid::new_v4()]);
    let mut relay_client1_2 = relay_client1.clone();
    update(
        &mut relay_client1,
        &account,
        &identity_key_details1,
        &app_domain,
        &client_id,
        notify_key,
        &notification_types,
    )
    .await;
    let subs1 = accept_watch_subscriptions_changed(
        &mut relay_client1_2,
        &notify_server_client_id,
        &identity_key_details1,
        &account,
        watch_topic_key1,
    )
    .await
    .unwrap();
    assert_eq!(subs1.len(), 1);
    assert_eq!(subs1[0].scope, notification_types);
    let subs2 = accept_watch_subscriptions_changed(
        &mut relay_client2,
        &notify_server_client_id,
        &identity_key_details2,
        &account,
        watch_topic_key2,
    )
    .await
    .unwrap();
    assert_eq!(subs2.len(), 1);
    assert_eq!(subs2[0].scope, notification_types);

    let mut relay_client1_2 = relay_client1.clone();
    delete(
        &mut relay_client1,
        &identity_key_details1,
        &app_domain,
        &client_id,
        &account,
        notify_key,
    )
    .await;
    let subs1 = accept_watch_subscriptions_changed(
        &mut relay_client1_2,
        &notify_server_client_id,
        &identity_key_details1,
        &account,
        watch_topic_key1,
    )
    .await
    .unwrap();
    assert!(subs1.is_empty());
    let subs2 = accept_watch_subscriptions_changed(
        &mut relay_client2,
        &notify_server_client_id,
        &identity_key_details2,
        &account,
        watch_topic_key2,
    )
    .await
    .unwrap();
    assert!(subs2.is_empty());
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn watch_subscriptions_multiple_clients_mjv_v1(notify_server: &NotifyServerContext) {
    let (account_signing_key, account) = generate_account();

    let keys_server = MockServer::start().await;
    let keys_server_url = keys_server.uri().parse::<Url>().unwrap();

    let project_id = ProjectId::generate();
    let app_domain = DidWeb::from_domain(format!("{project_id}.walletconnect.com"));

    let (key_agreement, _authentication, client_id) =
        subscribe_topic(&project_id, app_domain.clone(), &notify_server.url).await;

    let (identity_signing_key1, identity_public_key1) = generate_identity_key();
    let identity_key_details1 = IdentityKeyDetails {
        keys_server_url: keys_server_url.clone(),
        signing_key: identity_signing_key1,
        client_id: identity_public_key1.clone(),
    };
    register_mocked_identity_key(
        &keys_server,
        identity_public_key1.clone(),
        sign_cacao(
            &app_domain,
            &account,
            CacaoAuth::Statement(STATEMENT_THIS_DOMAIN.to_owned()),
            identity_public_key1.clone(),
            identity_key_details1.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
    )
    .await;

    let vars = get_vars();
    let mut relay_client1 = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    let (identity_signing_key2, identity_public_key2) = generate_identity_key();
    let identity_key_details2 = IdentityKeyDetails {
        keys_server_url,
        signing_key: identity_signing_key2,
        client_id: identity_public_key2.clone(),
    };
    register_mocked_identity_key(
        &keys_server,
        identity_public_key2.clone(),
        sign_cacao(
            &app_domain,
            &account,
            CacaoAuth::Statement(STATEMENT_THIS_DOMAIN.to_owned()),
            identity_public_key2.clone(),
            identity_key_details2.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
    )
    .await;

    let vars = get_vars();
    let mut relay_client2 = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    let (subs1, watch_topic_key1, notify_server_client_id) = watch_subscriptions(
        &mut relay_client1,
        notify_server.url.clone(),
        &identity_key_details1,
        Some(app_domain.clone()),
        &account,
    )
    .await
    .unwrap();
    assert!(subs1.is_empty());

    let (subs2, watch_topic_key2, notify_server_client_id2) = watch_subscriptions(
        &mut relay_client2,
        notify_server.url.clone(),
        &identity_key_details2,
        Some(app_domain.clone()),
        &account,
    )
    .await
    .unwrap();
    assert!(subs2.is_empty());
    assert_eq!(notify_server_client_id2, notify_server_client_id);

    let notification_type = Uuid::new_v4();
    let notification_types = HashSet::from([notification_type]);
    let mut relay_client1_2 = relay_client1.clone();
    let subs1 = subscribe_v1(
        &mut relay_client1,
        &account,
        &identity_key_details1,
        key_agreement,
        &client_id,
        app_domain.clone(),
        notification_types.clone(),
    )
    .await;
    assert_eq!(subs1.len(), 1);
    let sub1 = &subs1[0];
    assert_eq!(sub1.scope, notification_types);

    let subs2 = accept_watch_subscriptions_changed(
        &mut relay_client2,
        &notify_server_client_id,
        &identity_key_details2,
        &account,
        watch_topic_key2,
    )
    .await
    .unwrap();
    assert_eq!(subs2.len(), 1);
    let sub2 = &subs2[0];
    assert_eq!(sub2.scope, notification_types);

    let result1 = tokio::time::timeout(
        RELAY_MESSAGE_DELIVERY_TIMEOUT / 2,
        accept_watch_subscriptions_changed(
            &mut relay_client1_2,
            &notify_server_client_id,
            &identity_key_details1,
            &account,
            watch_topic_key1,
        ),
    )
    .await;
    assert!(result1.is_err());

    assert_eq!(sub1.sym_key, sub2.sym_key);
    let notify_key = decode_key(&sub2.sym_key).unwrap();

    relay_client1.subscribe(topic_from_key(&notify_key)).await;

    // Update to 2 types
    let notification_types = HashSet::from([Uuid::new_v4(), Uuid::new_v4()]);
    let mut relay_client1_2 = relay_client1.clone();
    let subs1 = update_v1(
        &mut relay_client1,
        &account,
        &identity_key_details1,
        &app_domain,
        &client_id,
        notify_key,
        &notification_types,
    )
    .await;
    assert_eq!(subs1.len(), 1);
    assert_eq!(subs1[0].scope, notification_types);

    let subs2 = accept_watch_subscriptions_changed(
        &mut relay_client2,
        &notify_server_client_id,
        &identity_key_details2,
        &account,
        watch_topic_key2,
    )
    .await
    .unwrap();
    assert_eq!(subs2.len(), 1);
    assert_eq!(subs2[0].scope, notification_types);

    let result1 = tokio::time::timeout(
        RELAY_MESSAGE_DELIVERY_TIMEOUT / 2,
        accept_watch_subscriptions_changed(
            &mut relay_client1_2,
            &notify_server_client_id,
            &identity_key_details1,
            &account,
            watch_topic_key1,
        ),
    )
    .await;
    assert!(result1.is_err());

    let mut relay_client1_2 = relay_client1.clone();
    let subs1 = delete_v1(
        &mut relay_client1,
        &identity_key_details1,
        &app_domain,
        &client_id,
        &account,
        notify_key,
    )
    .await;
    assert!(subs1.is_empty());

    let subs2 = accept_watch_subscriptions_changed(
        &mut relay_client2,
        &notify_server_client_id,
        &identity_key_details2,
        &account,
        watch_topic_key2,
    )
    .await
    .unwrap();
    assert!(subs2.is_empty());

    let result1 = tokio::time::timeout(
        RELAY_MESSAGE_DELIVERY_TIMEOUT / 2,
        accept_watch_subscriptions_changed(
            &mut relay_client1_2,
            &notify_server_client_id,
            &identity_key_details1,
            &account,
            watch_topic_key1,
        ),
    )
    .await;
    assert!(result1.is_err());
}

#[tokio::test]
pub async fn test_same_account() {
    let (postgres, _) = get_postgres().await;

    async fn test(is_same: bool, account1: &AccountId, account2: &AccountId, postgres: &PgPool) {
        assert_eq!(is_same, is_same_address(account1, account2));

        async fn query(account: &AccountId, postgres: &PgPool) -> String {
            #[derive(Debug, FromRow)]
            struct AddressResult {
                address: String,
            }
            let result = sqlx::query_as::<Postgres, AddressResult>(
                "SELECT get_address_lower($1) AS address",
            )
            .bind(account.as_ref())
            .fetch_one(postgres)
            .await
            .unwrap();
            result.address
        }
        assert_eq!(
            is_same,
            query(account1, postgres).await == query(account2, postgres).await
        );
    }

    let account = generate_account().1;
    test(true, &account, &account, &postgres).await;
    test(
        false,
        &generate_account().1,
        &generate_account().1,
        &postgres,
    )
    .await;
    let (_key, address) = generate_eoa();
    test(
        true,
        &format_eip155_account(1, &address),
        &format_eip155_account(1, &address),
        &postgres,
    )
    .await;
    test(
        true,
        &format_eip155_account(1, &address),
        &format_eip155_account(2, &address),
        &postgres,
    )
    .await;
    test(
        true,
        &format_eip155_account(1, &address),
        &format_eip155_account(22, &address),
        &postgres,
    )
    .await;
    test(
        true,
        &format_eip155_account(1, &address),
        &format_eip155_account(242, &address),
        &postgres,
    )
    .await;
    test(
        true,
        &AccountId::try_from("eip155:1:0x62639418051006514eD5Bb5B20aa7aAD642cC2d0").unwrap(),
        &AccountId::try_from("eip155:2:0x62639418051006514eD5Bb5B20aa7aAD642cC2d0").unwrap(),
        &postgres,
    )
    .await;
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn same_address_different_chain_modify_subscription(notify_server: &NotifyServerContext) {
    let project_id = ProjectId::generate();
    let app_domain = DidWeb::from_domain(format!("{project_id}.walletconnect.com"));

    let (account_signing_key, address) = generate_eoa();
    let account1 = format_eip155_account(1, &address);
    let account2 = format_eip155_account(2, &address);

    let keys_server = MockServer::start().await;
    let keys_server_url = keys_server.uri().parse::<Url>().unwrap();

    let (identity_signing_key1, identity_public_key1) = generate_identity_key();
    let identity_key_details1 = IdentityKeyDetails {
        keys_server_url: keys_server_url.clone(),
        signing_key: identity_signing_key1,
        client_id: identity_public_key1.clone(),
    };
    register_mocked_identity_key(
        &keys_server,
        identity_public_key1.clone(),
        sign_cacao(
            &app_domain,
            &account1,
            CacaoAuth::Statement(STATEMENT_THIS_DOMAIN.to_owned()),
            identity_public_key1.clone(),
            identity_key_details1.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
    )
    .await;

    let (identity_signing_key2, identity_public_key2) = generate_identity_key();
    let identity_key_details2 = IdentityKeyDetails {
        keys_server_url,
        signing_key: identity_signing_key2,
        client_id: identity_public_key2.clone(),
    };
    register_mocked_identity_key(
        &keys_server,
        identity_public_key2.clone(),
        sign_cacao(
            &app_domain,
            &account2,
            CacaoAuth::Statement(STATEMENT_THIS_DOMAIN.to_owned()),
            identity_public_key2.clone(),
            identity_key_details2.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
    )
    .await;

    let vars = get_vars();
    let mut relay_client = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    let (key_agreement, _authentication, client_id) =
        subscribe_topic(&project_id, app_domain.clone(), &notify_server.url).await;

    let (subs, watch_topic_key, notify_server_client_id) = watch_subscriptions(
        &mut relay_client,
        notify_server.url.clone(),
        &identity_key_details1,
        Some(app_domain.clone()),
        &account1,
    )
    .await
    .unwrap();
    assert!(subs.is_empty());

    // Subscribe with 1 type
    let notification_types = HashSet::from([Uuid::new_v4()]);
    let mut relay_client2 = relay_client.clone();
    subscribe(
        &mut relay_client,
        &account1,
        &identity_key_details1,
        key_agreement,
        &client_id,
        app_domain.clone(),
        notification_types.clone(),
    )
    .await
    .unwrap();
    let subs = accept_watch_subscriptions_changed(
        &mut relay_client2,
        &notify_server_client_id,
        &identity_key_details1,
        &account1,
        watch_topic_key,
    )
    .await
    .unwrap();
    assert_eq!(subs.len(), 1);
    let sub = &subs[0];
    assert_eq!(sub.scope, notification_types);

    let notify_key = decode_key(&sub.sym_key).unwrap();

    relay_client.subscribe(topic_from_key(&notify_key)).await;

    let notification_types = HashSet::from([]);
    let mut relay_client2 = relay_client.clone();
    update(
        &mut relay_client,
        &account2,
        &identity_key_details2,
        &app_domain,
        &client_id,
        notify_key,
        &notification_types,
    )
    .await;
    let subs = accept_watch_subscriptions_changed(
        &mut relay_client2,
        &notify_server_client_id,
        &identity_key_details1,
        &account1,
        watch_topic_key,
    )
    .await
    .unwrap();
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0].scope, notification_types);
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn same_address_different_chain_watch_subscriptions(notify_server: &NotifyServerContext) {
    let project_id = ProjectId::generate();
    let app_domain = DidWeb::from_domain(format!("{project_id}.walletconnect.com"));

    let (account_signing_key, address) = generate_eoa();
    let account1 = format_eip155_account(1, &address);
    let account2 = format_eip155_account(2, &address);

    let keys_server = MockServer::start().await;
    let keys_server_url = keys_server.uri().parse::<Url>().unwrap();

    let (identity_signing_key1, identity_public_key1) = generate_identity_key();
    let identity_key_details1 = IdentityKeyDetails {
        keys_server_url: keys_server_url.clone(),
        signing_key: identity_signing_key1,
        client_id: identity_public_key1.clone(),
    };
    register_mocked_identity_key(
        &keys_server,
        identity_public_key1.clone(),
        sign_cacao(
            &app_domain,
            &account1,
            CacaoAuth::Statement(STATEMENT_THIS_DOMAIN.to_owned()),
            identity_public_key1.clone(),
            identity_key_details1.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
    )
    .await;

    let (identity_signing_key2, identity_public_key2) = generate_identity_key();
    let identity_key_details2 = IdentityKeyDetails {
        keys_server_url,
        signing_key: identity_signing_key2,
        client_id: identity_public_key2.clone(),
    };
    register_mocked_identity_key(
        &keys_server,
        identity_public_key2.clone(),
        sign_cacao(
            &app_domain,
            &account2,
            CacaoAuth::Statement(STATEMENT_THIS_DOMAIN.to_owned()),
            identity_public_key2.clone(),
            identity_key_details2.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
    )
    .await;

    let vars = get_vars();
    let mut relay_client = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    let (key_agreement, _authentication, client_id) =
        subscribe_topic(&project_id, app_domain.clone(), &notify_server.url).await;

    let (subs1, watch_topic_key1, notify_server_client_id) = watch_subscriptions(
        &mut relay_client,
        notify_server.url.clone(),
        &identity_key_details1,
        Some(app_domain.clone()),
        &account1,
    )
    .await
    .unwrap();
    assert!(subs1.is_empty());

    let (subs2, watch_topic_key2, _notify_server_client_id) = watch_subscriptions(
        &mut relay_client,
        notify_server.url.clone(),
        &identity_key_details2,
        Some(app_domain.clone()),
        &account2,
    )
    .await
    .unwrap();
    assert!(subs2.is_empty());

    // Subscribe with 1 type
    let notification_types = HashSet::from([Uuid::new_v4()]);
    let mut relay_client2 = relay_client.clone();
    let mut relay_client3 = relay_client.clone();
    subscribe(
        &mut relay_client,
        &account1,
        &identity_key_details1,
        key_agreement,
        &client_id,
        app_domain.clone(),
        notification_types.clone(),
    )
    .await
    .unwrap();
    let subs = accept_watch_subscriptions_changed(
        &mut relay_client2,
        &notify_server_client_id,
        &identity_key_details1,
        &account1,
        watch_topic_key1,
    )
    .await
    .unwrap();
    assert_eq!(subs.len(), 1);
    let sub1 = &subs[0];
    assert_eq!(sub1.scope, notification_types);
    assert_eq!(sub1.account, account1);
    let subs = accept_watch_subscriptions_changed(
        &mut relay_client3,
        &notify_server_client_id,
        &identity_key_details2,
        &account2,
        watch_topic_key2,
    )
    .await
    .unwrap();
    assert_eq!(subs.len(), 1);
    let sub2 = &subs[0];
    assert_eq!(sub2.scope, notification_types);
    assert_eq!(sub2.account, account2);

    assert_eq!(sub1.sym_key, sub2.sym_key);
    let notify_key = decode_key(&sub2.sym_key).unwrap();

    relay_client.subscribe(topic_from_key(&notify_key)).await;

    let notification_types = HashSet::from([]);
    let mut relay_client2 = relay_client.clone();
    let mut relay_client3 = relay_client.clone();
    update(
        &mut relay_client,
        &account2,
        &identity_key_details2,
        &app_domain,
        &client_id,
        notify_key,
        &notification_types,
    )
    .await;
    let subs = accept_watch_subscriptions_changed(
        &mut relay_client2,
        &notify_server_client_id,
        &identity_key_details1,
        &account1,
        watch_topic_key1,
    )
    .await
    .unwrap();
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0].scope, notification_types);
    assert_eq!(subs[0].account, account1);
    let subs = accept_watch_subscriptions_changed(
        &mut relay_client3,
        &notify_server_client_id,
        &identity_key_details2,
        &account2,
        watch_topic_key2,
    )
    .await
    .unwrap();
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0].scope, notification_types);
    assert_eq!(subs[0].account, account2);
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn watch_subscriptions_response_chain_agnostic(notify_server: &NotifyServerContext) {
    let project_id = ProjectId::generate();
    let app_domain = DidWeb::from_domain(format!("{project_id}.walletconnect.com"));

    let (account_signing_key, address) = generate_eoa();
    let account1 = format_eip155_account(1, &address);
    let account2 = format_eip155_account(2, &address);

    let keys_server = MockServer::start().await;
    let keys_server_url = keys_server.uri().parse::<Url>().unwrap();

    let (identity_signing_key1, identity_public_key1) = generate_identity_key();
    let identity_key_details1 = IdentityKeyDetails {
        keys_server_url: keys_server_url.clone(),
        signing_key: identity_signing_key1,
        client_id: identity_public_key1.clone(),
    };
    register_mocked_identity_key(
        &keys_server,
        identity_public_key1.clone(),
        sign_cacao(
            &app_domain,
            &account1,
            CacaoAuth::Statement(STATEMENT_THIS_DOMAIN.to_owned()),
            identity_public_key1.clone(),
            identity_key_details1.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
    )
    .await;

    let (identity_signing_key2, identity_public_key2) = generate_identity_key();
    let identity_key_details2 = IdentityKeyDetails {
        keys_server_url,
        signing_key: identity_signing_key2,
        client_id: identity_public_key2.clone(),
    };
    register_mocked_identity_key(
        &keys_server,
        identity_public_key2.clone(),
        sign_cacao(
            &app_domain,
            &account2,
            CacaoAuth::Statement(STATEMENT_THIS_DOMAIN.to_owned()),
            identity_public_key2.clone(),
            identity_key_details2.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
    )
    .await;

    let vars = get_vars();
    let mut relay_client = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    let (key_agreement, _authentication, client_id) =
        subscribe_topic(&project_id, app_domain.clone(), &notify_server.url).await;

    let notification_types = HashSet::from([Uuid::new_v4()]);
    subscribe(
        &mut relay_client,
        &account1,
        &identity_key_details1,
        key_agreement,
        &client_id,
        app_domain.clone(),
        notification_types.clone(),
    )
    .await
    .unwrap();

    let (subs1, _watch_topic_key1, _notify_server_client_id) = watch_subscriptions(
        &mut relay_client,
        notify_server.url.clone(),
        &identity_key_details1,
        Some(app_domain.clone()),
        &account1,
    )
    .await
    .unwrap();
    assert_eq!(subs1.len(), 1);
    assert_eq!(subs1[0].scope, notification_types);
    assert_eq!(subs1[0].account, account1);

    let (subs2, _watch_topic_key2, _notify_server_client_id) = watch_subscriptions(
        &mut relay_client,
        notify_server.url.clone(),
        &identity_key_details2,
        Some(app_domain.clone()),
        &account2,
    )
    .await
    .unwrap();
    assert_eq!(subs2.len(), 1);
    assert_eq!(subs2[0].scope, notification_types);
    assert_eq!(subs2[0].account, account2);
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn no_watcher_gives_only_chains_for_subscription(notify_server: &NotifyServerContext) {
    let project_id1 = ProjectId::generate();
    let app_domain1 = DidWeb::from_domain(format!("{project_id1}.walletconnect.com"));

    let project_id2 = ProjectId::generate();
    let app_domain2 = DidWeb::from_domain(format!("{project_id2}.walletconnect.com"));

    let (account_signing_key, address) = generate_eoa();
    let account1 = format_eip155_account(1, &address);
    let account2 = format_eip155_account(2, &address);

    let keys_server = MockServer::start().await;
    let keys_server_url = keys_server.uri().parse::<Url>().unwrap();

    let (identity_signing_key1, identity_public_key1) = generate_identity_key();
    let identity_key_details1 = IdentityKeyDetails {
        keys_server_url: keys_server_url.clone(),
        signing_key: identity_signing_key1,
        client_id: identity_public_key1.clone(),
    };
    register_mocked_identity_key(
        &keys_server,
        identity_public_key1.clone(),
        sign_cacao(
            &DidWeb::from_domain("com.example.wallet".to_owned()),
            &account1,
            CacaoAuth::Statement(STATEMENT_ALL_DOMAINS.to_owned()),
            identity_public_key1.clone(),
            identity_key_details1.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
    )
    .await;

    let (identity_signing_key2, identity_public_key2) = generate_identity_key();
    let identity_key_details2 = IdentityKeyDetails {
        keys_server_url,
        signing_key: identity_signing_key2,
        client_id: identity_public_key2.clone(),
    };
    register_mocked_identity_key(
        &keys_server,
        identity_public_key2.clone(),
        sign_cacao(
            &DidWeb::from_domain("com.example.wallet".to_owned()),
            &account2,
            CacaoAuth::Statement(STATEMENT_ALL_DOMAINS.to_owned()),
            identity_public_key2.clone(),
            identity_key_details2.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
    )
    .await;

    let vars = get_vars();
    let mut relay_client = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    let (key_agreement1, _authentication, client_id1) =
        subscribe_topic(&project_id1, app_domain1.clone(), &notify_server.url).await;
    let (key_agreement2, _authentication, client_id2) =
        subscribe_topic(&project_id2, app_domain2.clone(), &notify_server.url).await;

    let subs = subscribe_v1(
        &mut relay_client,
        &account1,
        &identity_key_details1,
        key_agreement1,
        &client_id1,
        app_domain1.clone(),
        HashSet::from([Uuid::new_v4()]),
    )
    .await;
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0].account, account1);

    let subs = subscribe_v1(
        &mut relay_client,
        &account2,
        &identity_key_details2,
        key_agreement2,
        &client_id2,
        app_domain2.clone(),
        HashSet::from([Uuid::new_v4()]),
    )
    .await;
    assert_eq!(subs.len(), 1);
    assert_eq!(DidWeb::from_domain(subs[0].app_domain.clone()), app_domain2);
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn subscribe_response_chain_agnostic(notify_server: &NotifyServerContext) {
    let project_id1 = ProjectId::generate();
    let app_domain1 = DidWeb::from_domain(format!("{project_id1}.walletconnect.com"));

    let project_id2 = ProjectId::generate();
    let app_domain2 = DidWeb::from_domain(format!("{project_id2}.walletconnect.com"));

    let (account_signing_key, address) = generate_eoa();
    let account1 = format_eip155_account(1, &address);
    let account2 = format_eip155_account(2, &address);

    let keys_server = MockServer::start().await;
    let keys_server_url = keys_server.uri().parse::<Url>().unwrap();

    let (identity_signing_key1, identity_public_key1) = generate_identity_key();
    let identity_key_details1 = IdentityKeyDetails {
        keys_server_url: keys_server_url.clone(),
        signing_key: identity_signing_key1,
        client_id: identity_public_key1.clone(),
    };
    register_mocked_identity_key(
        &keys_server,
        identity_public_key1.clone(),
        sign_cacao(
            &DidWeb::from_domain("com.example.wallet".to_owned()),
            &account1,
            CacaoAuth::Statement(STATEMENT_ALL_DOMAINS.to_owned()),
            identity_public_key1.clone(),
            identity_key_details1.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
    )
    .await;

    let (identity_signing_key2, identity_public_key2) = generate_identity_key();
    let identity_key_details2 = IdentityKeyDetails {
        keys_server_url,
        signing_key: identity_signing_key2,
        client_id: identity_public_key2.clone(),
    };
    register_mocked_identity_key(
        &keys_server,
        identity_public_key2.clone(),
        sign_cacao(
            &DidWeb::from_domain("com.example.wallet".to_owned()),
            &account2,
            CacaoAuth::Statement(STATEMENT_ALL_DOMAINS.to_owned()),
            identity_public_key2.clone(),
            identity_key_details2.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
    )
    .await;

    let vars = get_vars();
    let mut relay_client = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    let (key_agreement1, _authentication, client_id1) =
        subscribe_topic(&project_id1, app_domain1.clone(), &notify_server.url).await;
    let (key_agreement2, _authentication, client_id2) =
        subscribe_topic(&project_id2, app_domain2.clone(), &notify_server.url).await;

    let (_subs, _watch_topic_key1, _notify_server_client_id) = watch_subscriptions(
        &mut relay_client,
        notify_server.url.clone(),
        &identity_key_details1,
        None,
        &account1,
    )
    .await
    .unwrap();
    let (_subs, _watch_topic_key1, _notify_server_client_id) = watch_subscriptions(
        &mut relay_client,
        notify_server.url.clone(),
        &identity_key_details2,
        None,
        &account2,
    )
    .await
    .unwrap();

    let subs = subscribe_v1(
        &mut relay_client,
        &account1,
        &identity_key_details1,
        key_agreement1,
        &client_id1,
        app_domain1.clone(),
        HashSet::from([Uuid::new_v4()]),
    )
    .await;
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0].account, account1);

    let subs = subscribe_v1(
        &mut relay_client,
        &account2,
        &identity_key_details2,
        key_agreement2,
        &client_id2,
        app_domain2.clone(),
        HashSet::from([Uuid::new_v4()]),
    )
    .await;
    assert_eq!(subs.len(), 2);
    assert_eq!(
        subs.iter()
            .find(|sub| DidWeb::from_domain(sub.app_domain.clone()) == app_domain1)
            .unwrap()
            .account,
        account2
    );
    assert_eq!(
        subs.iter()
            .find(|sub| DidWeb::from_domain(sub.app_domain.clone()) == app_domain2)
            .unwrap()
            .account,
        account2
    );
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn update_response_chain_agnostic(notify_server: &NotifyServerContext) {
    let project_id1 = ProjectId::generate();
    let app_domain1 = DidWeb::from_domain(format!("{project_id1}.walletconnect.com"));

    let project_id2 = ProjectId::generate();
    let app_domain2 = DidWeb::from_domain(format!("{project_id2}.walletconnect.com"));

    let (account_signing_key, address) = generate_eoa();
    let account1 = format_eip155_account(1, &address);
    let account2 = format_eip155_account(2, &address);

    let keys_server = MockServer::start().await;
    let keys_server_url = keys_server.uri().parse::<Url>().unwrap();

    let (identity_signing_key1, identity_public_key1) = generate_identity_key();
    let identity_key_details1 = IdentityKeyDetails {
        keys_server_url: keys_server_url.clone(),
        signing_key: identity_signing_key1,
        client_id: identity_public_key1.clone(),
    };
    register_mocked_identity_key(
        &keys_server,
        identity_public_key1.clone(),
        sign_cacao(
            &DidWeb::from_domain("com.example.wallet".to_owned()),
            &account1,
            CacaoAuth::Statement(STATEMENT_ALL_DOMAINS.to_owned()),
            identity_public_key1.clone(),
            identity_key_details1.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
    )
    .await;

    let (identity_signing_key2, identity_public_key2) = generate_identity_key();
    let identity_key_details2 = IdentityKeyDetails {
        keys_server_url,
        signing_key: identity_signing_key2,
        client_id: identity_public_key2.clone(),
    };
    register_mocked_identity_key(
        &keys_server,
        identity_public_key2.clone(),
        sign_cacao(
            &DidWeb::from_domain("com.example.wallet".to_owned()),
            &account2,
            CacaoAuth::Statement(STATEMENT_ALL_DOMAINS.to_owned()),
            identity_public_key2.clone(),
            identity_key_details2.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
    )
    .await;

    let vars = get_vars();
    let mut relay_client = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    let (key_agreement1, _authentication, app_client_id1) =
        subscribe_topic(&project_id1, app_domain1.clone(), &notify_server.url).await;
    let (key_agreement2, _authentication, app_client_id2) =
        subscribe_topic(&project_id2, app_domain2.clone(), &notify_server.url).await;

    let (_subs, _watch_topic_key1, _notify_server_client_id) = watch_subscriptions(
        &mut relay_client,
        notify_server.url.clone(),
        &identity_key_details1,
        None,
        &account1,
    )
    .await
    .unwrap();
    let (_subs, _watch_topic_key1, _notify_server_client_id) = watch_subscriptions(
        &mut relay_client,
        notify_server.url.clone(),
        &identity_key_details2,
        None,
        &account2,
    )
    .await
    .unwrap();

    let subs = subscribe_v1(
        &mut relay_client,
        &account1,
        &identity_key_details1,
        key_agreement1,
        &app_client_id1,
        app_domain1.clone(),
        HashSet::from([Uuid::new_v4()]),
    )
    .await;
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0].account, account1);

    let subs = subscribe_v1(
        &mut relay_client,
        &account2,
        &identity_key_details2,
        key_agreement2,
        &app_client_id2,
        app_domain2.clone(),
        HashSet::from([Uuid::new_v4()]),
    )
    .await;
    assert_eq!(subs.len(), 2);
    assert_eq!(
        subs.iter()
            .find(|sub| DidWeb::from_domain(sub.app_domain.clone()) == app_domain1)
            .unwrap()
            .account,
        account2
    );
    assert_eq!(
        subs.iter()
            .find(|sub| DidWeb::from_domain(sub.app_domain.clone()) == app_domain2)
            .unwrap()
            .account,
        account2
    );

    let sub2_key = decode_key(
        &subs
            .iter()
            .find(|sub| DidWeb::from_domain(sub.app_domain.clone()) == app_domain2)
            .unwrap()
            .sym_key,
    )
    .unwrap();
    relay_client.subscribe(topic_from_key(&sub2_key)).await;

    let subs = update_v1(
        &mut relay_client,
        &account2,
        &identity_key_details2,
        &app_domain2,
        &app_client_id2,
        sub2_key,
        &HashSet::from([Uuid::new_v4()]),
    )
    .await;
    assert_eq!(subs.len(), 2);
    assert_eq!(
        subs.iter()
            .find(|sub| DidWeb::from_domain(sub.app_domain.clone()) == app_domain1)
            .unwrap()
            .account,
        account2
    );
    assert_eq!(
        subs.iter()
            .find(|sub| DidWeb::from_domain(sub.app_domain.clone()) == app_domain2)
            .unwrap()
            .account,
        account2
    );
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn delete_response_chain_agnostic(notify_server: &NotifyServerContext) {
    let project_id1 = ProjectId::generate();
    let app_domain1 = DidWeb::from_domain(format!("{project_id1}.walletconnect.com"));

    let project_id2 = ProjectId::generate();
    let app_domain2 = DidWeb::from_domain(format!("{project_id2}.walletconnect.com"));

    let (account_signing_key, address) = generate_eoa();
    let account1 = format_eip155_account(1, &address);
    let account2 = format_eip155_account(2, &address);

    let keys_server = MockServer::start().await;
    let keys_server_url = keys_server.uri().parse::<Url>().unwrap();

    let (identity_signing_key1, identity_public_key1) = generate_identity_key();
    let identity_key_details1 = IdentityKeyDetails {
        keys_server_url: keys_server_url.clone(),
        signing_key: identity_signing_key1,
        client_id: identity_public_key1.clone(),
    };
    register_mocked_identity_key(
        &keys_server,
        identity_public_key1.clone(),
        sign_cacao(
            &DidWeb::from_domain("com.example.wallet".to_owned()),
            &account1,
            CacaoAuth::Statement(STATEMENT_ALL_DOMAINS.to_owned()),
            identity_public_key1.clone(),
            identity_key_details1.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
    )
    .await;

    let (identity_signing_key2, identity_public_key2) = generate_identity_key();
    let identity_key_details2 = IdentityKeyDetails {
        keys_server_url,
        signing_key: identity_signing_key2,
        client_id: identity_public_key2.clone(),
    };
    register_mocked_identity_key(
        &keys_server,
        identity_public_key2.clone(),
        sign_cacao(
            &DidWeb::from_domain("com.example.wallet".to_owned()),
            &account2,
            CacaoAuth::Statement(STATEMENT_ALL_DOMAINS.to_owned()),
            identity_public_key2.clone(),
            identity_key_details2.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
    )
    .await;

    let vars = get_vars();
    let mut relay_client = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    let (key_agreement1, _authentication, app_client_id1) =
        subscribe_topic(&project_id1, app_domain1.clone(), &notify_server.url).await;
    let (key_agreement2, _authentication, app_client_id2) =
        subscribe_topic(&project_id2, app_domain2.clone(), &notify_server.url).await;

    let (_subs, _watch_topic_key1, _notify_server_client_id) = watch_subscriptions(
        &mut relay_client,
        notify_server.url.clone(),
        &identity_key_details1,
        None,
        &account1,
    )
    .await
    .unwrap();
    let (_subs, _watch_topic_key1, _notify_server_client_id) = watch_subscriptions(
        &mut relay_client,
        notify_server.url.clone(),
        &identity_key_details2,
        None,
        &account2,
    )
    .await
    .unwrap();

    let subs = subscribe_v1(
        &mut relay_client,
        &account1,
        &identity_key_details1,
        key_agreement1,
        &app_client_id1,
        app_domain1.clone(),
        HashSet::from([Uuid::new_v4()]),
    )
    .await;
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0].account, account1);

    let subs = subscribe_v1(
        &mut relay_client,
        &account2,
        &identity_key_details2,
        key_agreement2,
        &app_client_id2,
        app_domain2.clone(),
        HashSet::from([Uuid::new_v4()]),
    )
    .await;
    assert_eq!(subs.len(), 2);
    assert_eq!(
        subs.iter()
            .find(|sub| DidWeb::from_domain(sub.app_domain.clone()) == app_domain1)
            .unwrap()
            .account,
        account2
    );
    assert_eq!(
        subs.iter()
            .find(|sub| DidWeb::from_domain(sub.app_domain.clone()) == app_domain2)
            .unwrap()
            .account,
        account2
    );

    let sub2_key = decode_key(
        &subs
            .iter()
            .find(|sub| DidWeb::from_domain(sub.app_domain.clone()) == app_domain2)
            .unwrap()
            .sym_key,
    )
    .unwrap();
    relay_client.subscribe(topic_from_key(&sub2_key)).await;

    let subs = delete_v1(
        &mut relay_client,
        &identity_key_details2,
        &app_domain2,
        &app_client_id2,
        &account2,
        sub2_key,
    )
    .await;
    assert_eq!(subs.len(), 1);
    assert_eq!(
        subs.iter()
            .find(|sub| DidWeb::from_domain(sub.app_domain.clone()) == app_domain1)
            .unwrap()
            .account,
        account2
    );
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn same_address_different_chain_notify(notify_server: &NotifyServerContext) {
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

    let (_account_signing_key, address) = generate_eoa();
    let account1: AccountId = format_eip155_account(1, &address);
    let account2: AccountId = format_eip155_account(2, &address);
    let notification_type = Uuid::new_v4();
    let scope = HashSet::from([notification_type]);
    let notify_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let notify_topic = topic_from_key(&notify_key);
    upsert_subscriber(
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

    let vars = get_vars();
    let mut relay_client = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    relay_client.subscribe(notify_topic.clone()).await;

    let notification = Notification {
        r#type: notification_type,
        title: "title".to_owned(),
        body: "body".to_owned(),
        icon: None,
        url: None,
    };

    let (_, address2) = generate_account();
    let notify_body = NotifyBody {
        notification_id: None,
        notification: notification.clone(),
        accounts: vec![account2.clone(), address2.clone()],
    };

    let response = assert_successful_response(
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
    .await
    .json::<notify_v1::ResponseBody>()
    .await
    .unwrap();
    assert_eq!(response.sent, HashSet::from([account2.clone()]));
    assert_eq!(response.not_found, HashSet::from([address2.clone()]));
    assert!(response.failed.is_empty());

    let (_, claims) = accept_notify_message(
        &mut relay_client,
        &account1,
        &authentication_key.verifying_key(),
        &get_client_id(&authentication_key.verifying_key()),
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
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn no_watcher_returns_only_app_subscriptions(notify_server: &NotifyServerContext) {
    let (account_signing_key, account) = generate_account();

    let keys_server = MockServer::start().await;
    let keys_server_url = keys_server.uri().parse::<Url>().unwrap();

    let (identity_signing_key, identity_public_key) = generate_identity_key();
    let identity_key_details = IdentityKeyDetails {
        keys_server_url: keys_server_url.clone(),
        signing_key: identity_signing_key,
        client_id: identity_public_key.clone(),
    };
    register_mocked_identity_key(
        &keys_server,
        identity_public_key.clone(),
        sign_cacao(
            &DidWeb::from_domain("com.example.wallet".to_owned()),
            &account,
            CacaoAuth::Statement(STATEMENT_ALL_DOMAINS.to_owned()),
            identity_public_key.clone(),
            identity_key_details.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
    )
    .await;

    let project_id1 = ProjectId::generate();
    let app_domain1 = DidWeb::from_domain(format!("{project_id1}.walletconnect.com"));
    let (key_agreement1, _authentication, client_id1) =
        subscribe_topic(&project_id1, app_domain1.clone(), &notify_server.url).await;

    let project_id2 = ProjectId::generate();
    let app_domain2 = DidWeb::from_domain(format!("{project_id2}.walletconnect.com"));
    let (key_agreement2, _authentication, client_id2) =
        subscribe_topic(&project_id2, app_domain2.clone(), &notify_server.url).await;

    let vars = get_vars();
    let mut relay_client = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    let notification_types = HashSet::from([Uuid::new_v4()]);
    let subs = subscribe_v1(
        &mut relay_client,
        &account,
        &identity_key_details,
        key_agreement1,
        &client_id1,
        app_domain1.clone(),
        notification_types.clone(),
    )
    .await;
    assert_eq!(subs.len(), 1);
    let sub = &subs[0];
    assert_eq!(sub.scope, notification_types);

    let notification_types = HashSet::from([Uuid::new_v4()]);
    let subs = subscribe_v1(
        &mut relay_client,
        &account,
        &identity_key_details,
        key_agreement2,
        &client_id2,
        app_domain2.clone(),
        notification_types.clone(),
    )
    .await;
    assert_eq!(subs.len(), 1);
    let sub = &subs[0];
    assert_eq!(sub.scope, notification_types);
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn different_account_subscribe_results_one_subscription(notify_server: &NotifyServerContext) {
    let project_id = ProjectId::generate();
    let app_domain = DidWeb::from_domain(format!("{project_id}.walletconnect.com"));

    let (account_signing_key, address) = generate_eoa();
    let account1 = format_eip155_account(1, &address);
    let account2 = format_eip155_account(2, &address);

    let keys_server = MockServer::start().await;
    let keys_server_url = keys_server.uri().parse::<Url>().unwrap();

    let (identity_signing_key1, identity_public_key1) = generate_identity_key();
    let identity_key_details1 = IdentityKeyDetails {
        keys_server_url: keys_server_url.clone(),
        signing_key: identity_signing_key1,
        client_id: identity_public_key1.clone(),
    };
    register_mocked_identity_key(
        &keys_server,
        identity_public_key1.clone(),
        sign_cacao(
            &app_domain,
            &account1,
            CacaoAuth::Statement(STATEMENT_THIS_DOMAIN.to_owned()),
            identity_public_key1.clone(),
            identity_key_details1.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
    )
    .await;

    let (identity_signing_key2, identity_public_key2) = generate_identity_key();
    let identity_key_details2 = IdentityKeyDetails {
        keys_server_url,
        signing_key: identity_signing_key2,
        client_id: identity_public_key2.clone(),
    };
    register_mocked_identity_key(
        &keys_server,
        identity_public_key2.clone(),
        sign_cacao(
            &app_domain,
            &account2,
            CacaoAuth::Statement(STATEMENT_THIS_DOMAIN.to_owned()),
            identity_public_key2.clone(),
            identity_key_details2.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
    )
    .await;

    let vars = get_vars();
    let mut relay_client = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    let (key_agreement, _authentication, client_id) =
        subscribe_topic(&project_id, app_domain.clone(), &notify_server.url).await;

    let (subs1, watch_topic_key1, notify_server_client_id) = watch_subscriptions(
        &mut relay_client,
        notify_server.url.clone(),
        &identity_key_details1,
        Some(app_domain.clone()),
        &account1,
    )
    .await
    .unwrap();
    assert!(subs1.is_empty());
    let (subs2, watch_topic_key2, _notify_server_client_id) = watch_subscriptions(
        &mut relay_client,
        notify_server.url.clone(),
        &identity_key_details2,
        Some(app_domain.clone()),
        &account2,
    )
    .await
    .unwrap();
    assert!(subs2.is_empty());

    let notification_types = HashSet::from([Uuid::new_v4()]);
    let mut relay_client2 = relay_client.clone();
    let mut relay_client3 = relay_client.clone();
    subscribe(
        &mut relay_client,
        &account1,
        &identity_key_details1,
        key_agreement,
        &client_id,
        app_domain.clone(),
        notification_types.clone(),
    )
    .await
    .unwrap();
    let subs1 = accept_watch_subscriptions_changed(
        &mut relay_client2,
        &notify_server_client_id,
        &identity_key_details1,
        &account1,
        watch_topic_key1,
    )
    .await
    .unwrap();
    assert_eq!(subs1.len(), 1);
    let sub1 = &subs1[0];
    assert_eq!(sub1.scope, notification_types);
    let subs2 = accept_watch_subscriptions_changed(
        &mut relay_client3,
        &notify_server_client_id,
        &identity_key_details2,
        &account2,
        watch_topic_key2,
    )
    .await
    .unwrap();
    assert_eq!(subs2.len(), 1);
    let sub2 = &subs2[0];
    assert_eq!(sub2.scope, notification_types);
    assert_eq!(sub1.sym_key, sub2.sym_key);

    let notification_types = HashSet::from([Uuid::new_v4()]);
    let mut relay_client2 = relay_client.clone();
    let mut relay_client3 = relay_client.clone();
    subscribe(
        &mut relay_client,
        &account2,
        &identity_key_details2,
        key_agreement,
        &client_id,
        app_domain.clone(),
        notification_types.clone(),
    )
    .await
    .unwrap();
    let subs1 = accept_watch_subscriptions_changed(
        &mut relay_client2,
        &notify_server_client_id,
        &identity_key_details1,
        &account1,
        watch_topic_key1,
    )
    .await
    .unwrap();
    assert_eq!(subs1.len(), 1);
    let sub1 = &subs1[0];
    assert_eq!(sub1.scope, notification_types);
    let subs2 = accept_watch_subscriptions_changed(
        &mut relay_client3,
        &notify_server_client_id,
        &identity_key_details2,
        &account2,
        watch_topic_key2,
    )
    .await
    .unwrap();
    assert_eq!(subs2.len(), 1);
    let sub2 = &subs2[0];
    assert_eq!(sub2.scope, notification_types);
    assert_eq!(sub1.sym_key, sub2.sym_key);
}

#[derive(Debug)]
struct StopMigrator {
    source: Migrator,
    stop_at: String,
}

impl StopMigrator {
    pub fn new(source: Migrator, stop_at: String) -> Self {
        Self { source, stop_at }
    }
}

impl<'s> MigrationSource<'s> for &'s StopMigrator {
    fn resolve(self) -> BoxFuture<'s, Result<Vec<Migration>, BoxDynError>> {
        let mut migrations = vec![];
        for migration in self.source.iter() {
            if format!(
                "{}_{}",
                migration.version,
                migration.description.replace(' ', "_")
            ) == self.stop_at
            {
                break;
            }
            migrations.push(migration.clone());
        }
        Box::pin(async move { Ok(migrations) })
    }
}

#[derive(Debug, FromRow)]
pub struct RawSubscribeResponse {
    pub id: Uuid,
    #[sqlx(try_from = "String")]
    pub account: AccountId,
}

pub async fn raw_upsert_subscriber(
    project: Uuid,
    account: AccountId,
    notify_key: &[u8; 32],
    notify_topic: Topic,
    postgres: &PgPool,
) -> Result<RawSubscribeResponse, sqlx::error::Error> {
    let query = "
        INSERT INTO subscriber (
            project,
            account,
            sym_key,
            topic,
            expiry
        )
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, account
    ";
    let subscriber = sqlx::query_as::<Postgres, RawSubscribeResponse>(query)
        .bind(project)
        .bind(account.as_ref())
        .bind(hex::encode(notify_key))
        .bind(notify_topic.as_ref())
        .bind(Utc::now() + chrono::Duration::days(30))
        .fetch_one(postgres)
        .await?;

    Ok(subscriber)
}

async fn prepare_duplicate_accounts_migration(postgres: &PgPool) {
    Migrator::new(&StopMigrator::new(
        sqlx::migrate!("./migrations"),
        "20240111200929_unique_account_address".to_string(),
    ))
    .await
    .unwrap()
    .run(postgres)
    .await
    .unwrap();
}

#[tokio::test]
async fn non_duplicated_accounts_are_ignored() {
    let (postgres, _) = get_postgres_without_migration().await;

    prepare_duplicate_accounts_migration(&postgres).await;

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

    let (_account_signing_key, address) = generate_eoa();
    let account1 = format_eip155_account(1, &address);

    let subscriber_sym_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let subscriber_topic = topic_from_key(&subscriber_sym_key);
    raw_upsert_subscriber(
        project.id,
        account1.clone(),
        &subscriber_sym_key,
        subscriber_topic.clone(),
        &postgres,
    )
    .await
    .unwrap();

    let accounts = get_subscriber_accounts_by_project_id(project_id.clone(), &postgres, None)
        .await
        .unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts, vec![account1.clone()]);

    sqlx::migrate!("./migrations").run(&postgres).await.unwrap();

    let accounts = get_subscriber_accounts_by_project_id(project_id.clone(), &postgres, None)
        .await
        .unwrap();
    assert_eq!(accounts.len(), 1);
    assert!(is_same_address(&accounts[0], &account1));
}

#[tokio::test]
async fn consolidate_2_accounts_to_one_address_migration() {
    let (postgres, _) = get_postgres_without_migration().await;

    prepare_duplicate_accounts_migration(&postgres).await;

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

    let (_account_signing_key, address) = generate_eoa();
    let account1 = format_eip155_account(1, &address);
    let account2 = format_eip155_account(2, &address);

    let subscriber_sym_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let subscriber_topic = topic_from_key(&subscriber_sym_key);
    raw_upsert_subscriber(
        project.id,
        account1.clone(),
        &subscriber_sym_key,
        subscriber_topic.clone(),
        &postgres,
    )
    .await
    .unwrap();

    let subscriber_sym_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let subscriber_topic = topic_from_key(&subscriber_sym_key);
    raw_upsert_subscriber(
        project.id,
        account2.clone(),
        &subscriber_sym_key,
        subscriber_topic.clone(),
        &postgres,
    )
    .await
    .unwrap();

    let accounts = get_subscriber_accounts_by_project_id(project_id.clone(), &postgres, None)
        .await
        .unwrap();
    assert_eq!(accounts.len(), 2);
    assert_eq!(
        accounts.into_iter().collect::<HashSet<_>>(),
        HashSet::from([account1.clone(), account2.clone()])
    );

    sqlx::migrate!("./migrations").run(&postgres).await.unwrap();

    let accounts = get_subscriber_accounts_by_project_id(project_id.clone(), &postgres, None)
        .await
        .unwrap();
    assert_eq!(accounts.len(), 1);
    assert!(is_same_address(&accounts[0], &account1));
    assert!(is_same_address(&accounts[0], &account2));
}

#[tokio::test]
async fn consolidate_3_accounts_to_one_address_migration() {
    let (postgres, _) = get_postgres_without_migration().await;

    prepare_duplicate_accounts_migration(&postgres).await;

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

    let (_account_signing_key, address) = generate_eoa();
    let account1 = format_eip155_account(1, &address);
    let account2 = format_eip155_account(2, &address);
    let account3 = format_eip155_account(3, &address);

    let subscriber_sym_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let subscriber_topic = topic_from_key(&subscriber_sym_key);
    raw_upsert_subscriber(
        project.id,
        account1.clone(),
        &subscriber_sym_key,
        subscriber_topic.clone(),
        &postgres,
    )
    .await
    .unwrap();

    let subscriber_sym_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let subscriber_topic = topic_from_key(&subscriber_sym_key);
    raw_upsert_subscriber(
        project.id,
        account2.clone(),
        &subscriber_sym_key,
        subscriber_topic.clone(),
        &postgres,
    )
    .await
    .unwrap();

    let subscriber_sym_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let subscriber_topic = topic_from_key(&subscriber_sym_key);
    raw_upsert_subscriber(
        project.id,
        account3.clone(),
        &subscriber_sym_key,
        subscriber_topic.clone(),
        &postgres,
    )
    .await
    .unwrap();

    let accounts = get_subscriber_accounts_by_project_id(project_id.clone(), &postgres, None)
        .await
        .unwrap();
    assert_eq!(accounts.len(), 3);
    assert_eq!(
        accounts.into_iter().collect::<HashSet<_>>(),
        HashSet::from([account1.clone(), account2.clone(), account3.clone()])
    );

    sqlx::migrate!("./migrations").run(&postgres).await.unwrap();

    let accounts = get_subscriber_accounts_by_project_id(project_id.clone(), &postgres, None)
        .await
        .unwrap();
    assert_eq!(accounts.len(), 1);
    assert!(is_same_address(&accounts[0], &account1));
    assert!(is_same_address(&accounts[0], &account2));
    assert!(is_same_address(&accounts[0], &account3));
}

#[tokio::test]
async fn deduplicate_two_addresses() {
    let (postgres, _) = get_postgres_without_migration().await;

    prepare_duplicate_accounts_migration(&postgres).await;

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

    let (_account_signing_key, address1) = generate_eoa();
    let account11 = format_eip155_account(1, &address1);
    let account21 = format_eip155_account(2, &address1);
    let (_account_signing_key, address2) = generate_eoa();
    let account12 = format_eip155_account(1, &address2);
    let account22 = format_eip155_account(2, &address2);

    let subscriber_sym_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let subscriber_topic = topic_from_key(&subscriber_sym_key);
    raw_upsert_subscriber(
        project.id,
        account11.clone(),
        &subscriber_sym_key,
        subscriber_topic.clone(),
        &postgres,
    )
    .await
    .unwrap();
    let subscriber_sym_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let subscriber_topic = topic_from_key(&subscriber_sym_key);
    raw_upsert_subscriber(
        project.id,
        account21.clone(),
        &subscriber_sym_key,
        subscriber_topic.clone(),
        &postgres,
    )
    .await
    .unwrap();

    let subscriber_sym_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let subscriber_topic = topic_from_key(&subscriber_sym_key);
    raw_upsert_subscriber(
        project.id,
        account12.clone(),
        &subscriber_sym_key,
        subscriber_topic.clone(),
        &postgres,
    )
    .await
    .unwrap();
    let subscriber_sym_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let subscriber_topic = topic_from_key(&subscriber_sym_key);
    raw_upsert_subscriber(
        project.id,
        account22.clone(),
        &subscriber_sym_key,
        subscriber_topic.clone(),
        &postgres,
    )
    .await
    .unwrap();

    let accounts = get_subscriber_accounts_by_project_id(project_id.clone(), &postgres, None)
        .await
        .unwrap();
    assert_eq!(accounts.len(), 4);
    assert_eq!(
        accounts.into_iter().collect::<HashSet<_>>(),
        HashSet::from([
            account11.clone(),
            account21.clone(),
            account12.clone(),
            account22.clone()
        ])
    );

    sqlx::migrate!("./migrations").run(&postgres).await.unwrap();

    let accounts = get_subscriber_accounts_by_project_id(project_id.clone(), &postgres, None)
        .await
        .unwrap();
    assert_eq!(accounts.len(), 2);
    if is_same_address(&accounts[0], &account11) {
        assert!(is_same_address(&accounts[0], &account11));
        assert!(is_same_address(&accounts[0], &account21));
        assert!(is_same_address(&accounts[1], &account12));
        assert!(is_same_address(&accounts[1], &account22));
    } else {
        assert!(is_same_address(&accounts[1], &account11));
        assert!(is_same_address(&accounts[1], &account21));
        assert!(is_same_address(&accounts[0], &account12));
        assert!(is_same_address(&accounts[0], &account22));
    }
}

#[tokio::test]
async fn two_accounts_not_consolidated_because_different_projects() {
    let (postgres, _) = get_postgres_without_migration().await;

    prepare_duplicate_accounts_migration(&postgres).await;

    let topic1 = Topic::generate();
    let project_id1 = ProjectId::generate();
    let subscribe_key1 = generate_subscribe_key();
    let authentication_key1 = generate_authentication_key();
    let app_domain1 = generate_app_domain();
    upsert_project(
        project_id1.clone(),
        &app_domain1,
        topic1,
        &authentication_key1,
        &subscribe_key1,
        &postgres,
        None,
    )
    .await
    .unwrap();
    let project1 = get_project_by_project_id(project_id1.clone(), &postgres, None)
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

    let (_account_signing_key, address) = generate_eoa();
    let account1 = format_eip155_account(1, &address);
    let account2 = format_eip155_account(2, &address);

    let subscriber_sym_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let subscriber_topic = topic_from_key(&subscriber_sym_key);
    raw_upsert_subscriber(
        project1.id,
        account1.clone(),
        &subscriber_sym_key,
        subscriber_topic.clone(),
        &postgres,
    )
    .await
    .unwrap();

    let subscriber_sym_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let subscriber_topic = topic_from_key(&subscriber_sym_key);
    raw_upsert_subscriber(
        project2.id,
        account2.clone(),
        &subscriber_sym_key,
        subscriber_topic.clone(),
        &postgres,
    )
    .await
    .unwrap();

    let accounts1 = get_subscriber_accounts_by_project_id(project_id1.clone(), &postgres, None)
        .await
        .unwrap();
    assert_eq!(accounts1.len(), 1);
    assert_eq!(accounts1[0], account1);
    let accounts2 = get_subscriber_accounts_by_project_id(project_id2.clone(), &postgres, None)
        .await
        .unwrap();
    assert_eq!(accounts2.len(), 1);
    assert_eq!(accounts2[0], account2);

    sqlx::migrate!("./migrations").run(&postgres).await.unwrap();

    let accounts1 = get_subscriber_accounts_by_project_id(project_id1.clone(), &postgres, None)
        .await
        .unwrap();
    assert_eq!(accounts1.len(), 1);
    assert_eq!(accounts1[0], account1);
    let accounts2 = get_subscriber_accounts_by_project_id(project_id2.clone(), &postgres, None)
        .await
        .unwrap();
    assert_eq!(accounts2.len(), 1);
    assert_eq!(accounts2[0], account2);
}

#[tokio::test]
async fn account_most_messages_kept() {
    let (postgres, _) = get_postgres_without_migration().await;

    prepare_duplicate_accounts_migration(&postgres).await;

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

    let (_account_signing_key, address) = generate_eoa();
    let account1 = format_eip155_account(1, &address);
    let account2 = format_eip155_account(2, &address);

    let subscriber_sym_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let subscriber_topic = topic_from_key(&subscriber_sym_key);
    let _subscriber1 = raw_upsert_subscriber(
        project.id,
        account1.clone(),
        &subscriber_sym_key,
        subscriber_topic.clone(),
        &postgres,
    )
    .await
    .unwrap();

    let subscriber_sym_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let subscriber_topic = topic_from_key(&subscriber_sym_key);
    let subscriber2 = raw_upsert_subscriber(
        project.id,
        account2.clone(),
        &subscriber_sym_key,
        subscriber_topic.clone(),
        &postgres,
    )
    .await
    .unwrap();

    let notification_with_id = upsert_notification(
        Uuid::new_v4().to_string(),
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

    upsert_subscriber_notifications(notification_with_id.id, &[subscriber2.id], &postgres, None)
        .await
        .unwrap();

    sqlx::migrate!("./migrations").run(&postgres).await.unwrap();

    let accounts = get_subscriber_accounts_by_project_id(project_id.clone(), &postgres, None)
        .await
        .unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(&accounts[0], &account2);
}

#[tokio::test]
async fn account_most_messages_kept2() {
    let (postgres, _) = get_postgres_without_migration().await;

    prepare_duplicate_accounts_migration(&postgres).await;

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

    let (_account_signing_key, address) = generate_eoa();
    let account1 = format_eip155_account(1, &address);
    let account2 = format_eip155_account(2, &address);

    let subscriber_sym_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let subscriber_topic = topic_from_key(&subscriber_sym_key);
    let subscriber1 = raw_upsert_subscriber(
        project.id,
        account1.clone(),
        &subscriber_sym_key,
        subscriber_topic.clone(),
        &postgres,
    )
    .await
    .unwrap();

    let subscriber_sym_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let subscriber_topic = topic_from_key(&subscriber_sym_key);
    let subscriber2 = raw_upsert_subscriber(
        project.id,
        account2.clone(),
        &subscriber_sym_key,
        subscriber_topic.clone(),
        &postgres,
    )
    .await
    .unwrap();

    let notification_with_id = upsert_notification(
        Uuid::new_v4().to_string(),
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
    upsert_subscriber_notifications(notification_with_id.id, &[subscriber1.id], &postgres, None)
        .await
        .unwrap();
    upsert_subscriber_notifications(notification_with_id.id, &[subscriber2.id], &postgres, None)
        .await
        .unwrap();

    let notification_with_id = upsert_notification(
        Uuid::new_v4().to_string(),
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
    upsert_subscriber_notifications(notification_with_id.id, &[subscriber2.id], &postgres, None)
        .await
        .unwrap();

    sqlx::migrate!("./migrations").run(&postgres).await.unwrap();

    let accounts = get_subscriber_accounts_by_project_id(project_id.clone(), &postgres, None)
        .await
        .unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(&accounts[0], &account2);
}

async fn run_test_duplicate_address_migration(
    subscribers: Vec<(&ProjectId, &AccountId)>,
    notifications: Vec<(&ProjectId, &AccountId)>,
    expected_subscribers: HashMap<&ProjectId, Vec<&AccountId>>,
) {
    let (postgres, _) = get_postgres_without_migration().await;

    prepare_duplicate_accounts_migration(&postgres).await;

    let mut inserted_projects = HashMap::new();
    for project_id in subscribers.iter().map(|(project, _account)| project) {
        let topic = Topic::generate();
        let subscribe_key = generate_subscribe_key();
        let authentication_key = generate_authentication_key();
        let app_domain = generate_app_domain();
        upsert_project(
            (*project_id).clone(),
            &app_domain,
            topic,
            &authentication_key,
            &subscribe_key,
            &postgres,
            None,
        )
        .await
        .unwrap();
        let project = get_project_by_project_id((*project_id).clone(), &postgres, None)
            .await
            .unwrap();

        inserted_projects.insert(project_id, project);
    }

    let mut inserted_subscribers = HashMap::new();
    for (project_id, account) in subscribers.iter() {
        let subscriber_sym_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
        let subscriber_topic = topic_from_key(&subscriber_sym_key);
        let subscriber = raw_upsert_subscriber(
            inserted_projects[project_id].id,
            (*account).clone(),
            &subscriber_sym_key,
            subscriber_topic.clone(),
            &postgres,
        )
        .await
        .unwrap();

        inserted_subscribers.insert((project_id, account), subscriber);
    }

    for (project_id, account) in notifications {
        let notification_with_id = upsert_notification(
            Uuid::new_v4().to_string(),
            inserted_projects[&project_id].id,
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
        upsert_subscriber_notifications(
            notification_with_id.id,
            &[inserted_subscribers[&(&project_id, &account)].id],
            &postgres,
            None,
        )
        .await
        .unwrap();
    }

    for project_id in subscribers.iter().map(|(project, _account)| project) {
        let accounts =
            get_subscriber_accounts_by_project_id((*project_id).clone(), &postgres, None)
                .await
                .unwrap();
        assert_eq!(
            accounts.into_iter().collect::<HashSet<_>>(),
            subscribers
                .iter()
                .filter(|(project, _account)| project == project_id)
                .map(|(_project, account)| (*account).clone())
                .collect::<HashSet<_>>()
        );
    }

    sqlx::migrate!("./migrations").run(&postgres).await.unwrap();

    for (project_id, expected_accounts) in expected_subscribers {
        let accounts = get_subscriber_accounts_by_project_id(project_id.clone(), &postgres, None)
            .await
            .unwrap();
        assert_eq!(
            accounts.into_iter().collect::<HashSet<_>>(),
            expected_accounts
                .into_iter()
                .cloned()
                .collect::<HashSet<_>>(),
        );
    }
}

#[tokio::test]
async fn test_duplicate_address_migration() {
    let project1 = &ProjectId::generate();
    let project2 = &ProjectId::generate();
    let project3 = &ProjectId::generate();

    let (_account_signing_key, address) = generate_eoa();
    let account1 = &format_eip155_account(1, &address);
    let account2 = &format_eip155_account(2, &address);
    let account3 = &format_eip155_account(3, &address);

    run_test_duplicate_address_migration(vec![], vec![], HashMap::from([])).await;
    run_test_duplicate_address_migration(
        vec![(project1, account1)],
        vec![],
        HashMap::from([(project1, vec![account1])]),
    )
    .await;
    run_test_duplicate_address_migration(
        vec![(project1, account1)],
        vec![(project1, account1), (project1, account1)],
        HashMap::from([(project1, vec![account1])]),
    )
    .await;
    run_test_duplicate_address_migration(
        vec![(project1, account1)],
        vec![
            (project1, account1),
            (project1, account1),
            (project1, account1),
        ],
        HashMap::from([(project1, vec![account1])]),
    )
    .await;
    run_test_duplicate_address_migration(
        vec![(project1, account1), (project1, account2)],
        vec![(project1, account1)],
        HashMap::from([(project1, vec![account1])]),
    )
    .await;
    run_test_duplicate_address_migration(
        vec![(project1, account1), (project1, account2)],
        vec![(project1, account2)],
        HashMap::from([(project1, vec![account2])]),
    )
    .await;
    run_test_duplicate_address_migration(
        vec![
            (project1, account1),
            (project1, account2),
            (project1, account3),
        ],
        vec![(project1, account2)],
        HashMap::from([(project1, vec![account2])]),
    )
    .await;
    run_test_duplicate_address_migration(
        vec![
            (project1, account1),
            (project1, account2),
            (project1, account3),
        ],
        vec![(project1, account1)],
        HashMap::from([(project1, vec![account1])]),
    )
    .await;
    run_test_duplicate_address_migration(
        vec![
            (project1, account1),
            (project1, account2),
            (project1, account3),
        ],
        vec![(project1, account2)],
        HashMap::from([(project1, vec![account2])]),
    )
    .await;
    run_test_duplicate_address_migration(
        vec![
            (project1, account1),
            (project1, account2),
            (project1, account3),
        ],
        vec![(project1, account3)],
        HashMap::from([(project1, vec![account3])]),
    )
    .await;
    run_test_duplicate_address_migration(
        vec![
            (project1, account1),
            (project1, account2),
            (project1, account3),
        ],
        vec![
            (project1, account1),
            (project1, account2),
            (project1, account2),
        ],
        HashMap::from([(project1, vec![account2])]),
    )
    .await;
    run_test_duplicate_address_migration(
        vec![
            (project1, account1),
            (project1, account2),
            (project1, account3),
        ],
        vec![
            (project1, account2),
            (project1, account2),
            (project1, account3),
            (project1, account3),
            (project1, account3),
        ],
        HashMap::from([(project1, vec![account3])]),
    )
    .await;
    run_test_duplicate_address_migration(
        vec![
            (project1, account1),
            (project1, account2),
            (project1, account3),
        ],
        vec![
            (project1, account1),
            (project1, account1),
            (project1, account1),
            (project1, account3),
            (project1, account3),
        ],
        HashMap::from([(project1, vec![account1])]),
    )
    .await;
    run_test_duplicate_address_migration(
        vec![
            (project1, account1),
            (project1, account2),
            (project1, account3),
            (project2, account1),
            (project2, account2),
            (project2, account3),
        ],
        vec![
            (project1, account1),
            (project1, account1),
            (project1, account1),
            (project1, account3),
            (project1, account3),
            (project2, account1),
            (project2, account1),
            (project2, account1),
            (project2, account3),
            (project2, account3),
        ],
        HashMap::from([(project1, vec![account1]), (project2, vec![account1])]),
    )
    .await;
    run_test_duplicate_address_migration(
        vec![
            (project1, account1),
            (project2, account1),
            (project3, account1),
        ],
        vec![],
        HashMap::from([
            (project1, vec![account1]),
            (project2, vec![account1]),
            (project3, vec![account1]),
        ]),
    )
    .await;
    run_test_duplicate_address_migration(
        vec![
            (project1, account1),
            (project2, account1),
            (project3, account1),
            (project1, account2),
            (project2, account2),
            (project3, account2),
        ],
        vec![
            (project1, account1),
            (project2, account1),
            (project3, account1),
            (project1, account2),
            (project2, account2),
            (project3, account2),
            (project1, account2),
            (project2, account2),
            (project3, account2),
        ],
        HashMap::from([
            (project1, vec![account2]),
            (project2, vec![account2]),
            (project3, vec![account2]),
        ]),
    )
    .await;
    run_test_duplicate_address_migration(
        vec![
            (project1, account1),
            (project2, account1),
            (project3, account1),
            (project1, account2),
            (project2, account2),
            (project3, account2),
        ],
        vec![
            (project1, account2),
            (project2, account2),
            (project3, account2),
            (project1, account2),
            (project2, account2),
            (project3, account2),
        ],
        HashMap::from([
            (project1, vec![account2]),
            (project2, vec![account2]),
            (project3, vec![account2]),
        ]),
    )
    .await;
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn relay_webhook_rejects_invalid_jwt(notify_server: &NotifyServerContext) {
    let response = reqwest::Client::new()
        .post(notify_server.url.join(RELAY_WEBHOOK_ENDPOINT).unwrap())
        .json(&WatchWebhookPayload {
            event_auth: vec!["".to_string()],
        })
        .send()
        .await
        .unwrap();
    let status = response.status();
    if status != StatusCode::UNPROCESSABLE_ENTITY {
        panic!(
            "expected unprocessable entity response, got {status}: {:?}",
            response.text().await
        );
    }
    let body = response.json::<Value>().await.unwrap();
    assert_eq!(
        body,
        json!({
            "error": "Could not parse watch event claims: Invalid format",
        })
    );
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn relay_webhook_rejects_wrong_aud(notify_server: &NotifyServerContext) {
    let webhook_url = notify_server.url.join(RELAY_WEBHOOK_ENDPOINT).unwrap();
    let keypair = SigningKey::generate(&mut rand::thread_rng());
    let payload = WatchWebhookPayload {
        event_auth: vec![WatchEventClaims {
            basic: JwtBasicClaims {
                iss: DidKey::from(DecodedClientId::from_key(&keypair.verifying_key())),
                aud: "example.com".to_owned(),
                // sub: DecodedClientId::from_key(&notify_server.keypair.public_key()).to_did_key(),
                sub: "".to_string(),
                iat: chrono::Utc::now().timestamp(),
                exp: Some(chrono::Utc::now().timestamp() + 60),
            },
            act: WatchAction::WatchEvent,
            typ: WatchType::Subscriber,
            whu: webhook_url.to_string(),
            evt: WatchEventPayload {
                message_id: serde_json::from_str("0").unwrap(),
                status: WatchStatus::Queued,
                topic: Topic::generate(),
                message: "message".to_owned().into(),
                published_at: 0,
                tag: 0,
            },
        }
        .encode(&keypair)
        .unwrap()],
    };
    let response = reqwest::Client::new()
        .post(webhook_url)
        .json(&payload)
        .send()
        .await
        .unwrap();
    let status = response.status();
    if status != StatusCode::UNPROCESSABLE_ENTITY {
        panic!(
            "expected unprocessable entity response, got {status}: {:?}",
            response.text().await
        );
    }
    let body = response.json::<Value>().await.unwrap();
    assert_eq!(
        body,
        json!({
            "error": "Could not verify watch event: Invalid audience",
        })
    );
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn relay_webhook_rejects_invalid_signature(notify_server: &NotifyServerContext) {
    let webhook_url = notify_server.url.join(RELAY_WEBHOOK_ENDPOINT).unwrap();
    let keypair1 = SigningKey::generate(&mut rand::thread_rng());
    let keypair2 = SigningKey::generate(&mut rand::thread_rng());
    let claims = WatchEventClaims {
        basic: JwtBasicClaims {
            iss: DidKey::from(DecodedClientId::from_key(&keypair1.verifying_key())),
            aud: notify_server.url.to_string(),
            // sub: DecodedClientId::from_key(&notify_server.keypair.public_key()).to_did_key(),
            sub: "".to_string(),
            iat: chrono::Utc::now().timestamp(),
            exp: Some(chrono::Utc::now().timestamp() + 60),
        },
        act: WatchAction::WatchEvent,
        typ: WatchType::Subscriber,
        whu: webhook_url.to_string(),
        evt: WatchEventPayload {
            message_id: serde_json::from_str("0").unwrap(),
            status: WatchStatus::Queued,
            topic: Topic::generate(),
            message: "message".to_owned().into(),
            published_at: 0,
            tag: 0,
        },
    };
    let event_auth = {
        let encoder = &data_encoding::BASE64URL_NOPAD;
        let header = encoder.encode(
            serde_json::to_string(&JwtHeader::default())
                .unwrap()
                .as_bytes(),
        );
        let claims = encoder.encode(serde_json::to_string(&claims).unwrap().as_bytes());
        let message = format!("{header}.{claims}");
        let signature = encoder.encode(&keypair2.sign(message.as_bytes()).to_bytes());
        format!("{message}.{signature}")
    };
    let payload = WatchWebhookPayload {
        event_auth: vec![event_auth],
    };
    let response = reqwest::Client::new()
        .post(webhook_url)
        .json(&payload)
        .send()
        .await
        .unwrap();
    let status = response.status();
    if status != StatusCode::UNPROCESSABLE_ENTITY {
        panic!(
            "expected unprocessable entity response, got {status}: {:?}",
            response.text().await
        );
    }
    let body = response.json::<Value>().await.unwrap();
    assert_eq!(
        body,
        json!({
            "error": "Could not parse watch event claims: Invalid signature",
        })
    );
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn relay_webhook_rejects_wrong_iss(notify_server: &NotifyServerContext) {
    let webhook_url = notify_server.url.join(RELAY_WEBHOOK_ENDPOINT).unwrap();
    let keypair = SigningKey::generate(&mut rand::thread_rng());
    let payload = WatchWebhookPayload {
        event_auth: vec![WatchEventClaims {
            basic: JwtBasicClaims {
                iss: DidKey::from(DecodedClientId::from_key(&keypair.verifying_key())),
                aud: notify_server.url.to_string(),
                // sub: DecodedClientId::from_key(&notify_server.keypair.public_key()).to_did_key(),
                sub: "".to_string(),
                iat: chrono::Utc::now().timestamp(),
                exp: Some(chrono::Utc::now().timestamp() + 60),
            },
            act: WatchAction::WatchEvent,
            typ: WatchType::Subscriber,
            whu: webhook_url.to_string(),
            evt: WatchEventPayload {
                message_id: serde_json::from_str("0").unwrap(),
                status: WatchStatus::Queued,
                topic: Topic::generate(),
                message: "message".to_owned().into(),
                published_at: 0,
                tag: 0,
            },
        }
        .encode(&keypair)
        .unwrap()],
    };
    let response = reqwest::Client::new()
        .post(webhook_url)
        .json(&payload)
        .send()
        .await
        .unwrap();
    let status = response.status();
    if status != StatusCode::UNPROCESSABLE_ENTITY {
        panic!(
            "expected unprocessable entity response, got {status}: {:?}",
            response.text().await
        );
    }
    let body = response.json::<Value>().await.unwrap();
    assert_eq!(
        body,
        json!({
            "error": "JWT has wrong issuer",
        })
    );
}

// TODO test wrong sub gives error
// TODO test wrong act gives error
// TODO test wrong typ gives error
// TODO test wrong whu gives error
// TODO test wrong status gives error

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn batch_receive_called(notify_server: &NotifyServerContext) {
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
    let app_domain = DidWeb::from_domain(format!("{project_id1}.example.com"));
    let (key_agreement, _authentication1, app_client_id) =
        subscribe_topic(&project_id1, app_domain.clone(), &notify_server.url).await;

    register_mocked_identity_key(
        &keys_server,
        identity_public_key.clone(),
        sign_cacao(
            &app_domain,
            &account,
            CacaoAuth::Statement(STATEMENT_THIS_DOMAIN.to_owned()),
            identity_public_key.clone(),
            identity_key_details.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
    )
    .await;

    let vars = get_vars();
    let mut relay_client = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.clone().into(),
        notify_server.url.clone(),
    )
    .await;

    subscribe(
        &mut relay_client,
        &account,
        &identity_key_details,
        key_agreement,
        &app_client_id,
        app_domain.clone(),
        HashSet::from([Uuid::new_v4()]),
    )
    .await
    .unwrap();

    tokio::time::sleep(BATCH_TIMEOUT + std::time::Duration::from_secs(1)).await;

    // Cannot poll because .fetch() also removes the messages

    let notify_server_relay_client = {
        let keypair_seed =
            decode_key(&sha256::digest(notify_server.keypair_seed.as_bytes())).unwrap();
        let keypair = SigningKey::generate(&mut StdRng::from_seed(keypair_seed));

        create_http_client(
            &keypair,
            vars.relay_url.parse().unwrap(),
            notify_server.url.clone(),
            vars.project_id.into(),
        )
        .unwrap()
    };

    let response = notify_server_relay_client
        .fetch(topic_from_key(key_agreement.as_bytes()))
        .await
        .unwrap();
    println!("fetch response: {response:?}");
    assert_eq!(response.messages.len(), 0);
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn subscription_watcher_limit(notify_server: &NotifyServerContext) {
    let (account_signing_key, account) = generate_account();

    let keys_server = MockServer::start().await;
    let keys_server_url = keys_server.uri().parse::<Url>().unwrap();
    let keys_server = Arc::new(keys_server);

    let project_id = ProjectId::generate();
    let app_domain = DidWeb::from_domain(format!("{project_id}.walletconnect.com"));

    let (_key_agreement, _authentication, _client_id) =
        subscribe_topic(&project_id, app_domain.clone(), &notify_server.url).await;

    futures_util::stream::iter(0..SUBSCRIPTION_WATCHER_LIMIT)
        .map(|_| {
            let keys_server = keys_server.clone();
            let keys_server_url = keys_server_url.clone();
            let account_signing_key = account_signing_key.clone();
            let app_domain = app_domain.clone();
            let account = account.clone();
            async move {
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
                        &app_domain,
                        &account,
                        CacaoAuth::Statement(STATEMENT_THIS_DOMAIN.to_owned()),
                        identity_public_key.clone(),
                        identity_key_details.keys_server_url.to_string(),
                        &account_signing_key,
                    )
                    .await,
                )
                .await;
                let vars = get_vars();
                let mut relay_client = RelayClient::new(
                    vars.relay_url.parse().unwrap(),
                    vars.project_id.into(),
                    notify_server.url.clone(),
                )
                .await;
                watch_subscriptions(
                    &mut relay_client,
                    notify_server.url.clone(),
                    &identity_key_details,
                    Some(app_domain),
                    &account,
                )
                .await
            }
        })
        .buffer_unordered(10)
        .collect::<Vec<_>>()
        .await;

    let (identity_signing_key, identity_public_key) = generate_identity_key();
    let identity_key_details = IdentityKeyDetails {
        keys_server_url: keys_server_url.clone(),
        signing_key: identity_signing_key,
        client_id: identity_public_key.clone(),
    };
    register_mocked_identity_key(
        &keys_server,
        identity_public_key.clone(),
        sign_cacao(
            &app_domain,
            &account,
            CacaoAuth::Statement(STATEMENT_THIS_DOMAIN.to_owned()),
            identity_public_key.clone(),
            identity_key_details.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
    )
    .await;
    let vars = get_vars();
    let mut relay_client = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;
    let result = watch_subscriptions(
        &mut relay_client,
        notify_server.url.clone(),
        &identity_key_details,
        Some(app_domain),
        &account,
    )
    .await;
    assert!(result.is_err());

    // Separate limit for different app domains
    let project_id = ProjectId::generate();
    let app_domain = DidWeb::from_domain(format!("{project_id}.walletconnect.com"));

    let (_key_agreement, _authentication, _client_id) =
        subscribe_topic(&project_id, app_domain.clone(), &notify_server.url).await;

    futures_util::stream::iter(0..SUBSCRIPTION_WATCHER_LIMIT)
        .map(|_| {
            let keys_server = keys_server.clone();
            let keys_server_url = keys_server_url.clone();
            let account_signing_key = account_signing_key.clone();
            let app_domain = app_domain.clone();
            let account = account.clone();
            async move {
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
                        &app_domain,
                        &account,
                        CacaoAuth::Statement(STATEMENT_THIS_DOMAIN.to_owned()),
                        identity_public_key.clone(),
                        identity_key_details.keys_server_url.to_string(),
                        &account_signing_key,
                    )
                    .await,
                )
                .await;
                let vars = get_vars();
                let mut relay_client = RelayClient::new(
                    vars.relay_url.parse().unwrap(),
                    vars.project_id.into(),
                    notify_server.url.clone(),
                )
                .await;
                watch_subscriptions(
                    &mut relay_client,
                    notify_server.url.clone(),
                    &identity_key_details,
                    Some(app_domain),
                    &account,
                )
                .await
            }
        })
        .buffer_unordered(10)
        .collect::<Vec<_>>()
        .await;

    let (identity_signing_key, identity_public_key) = generate_identity_key();
    let identity_key_details = IdentityKeyDetails {
        keys_server_url: keys_server_url.clone(),
        signing_key: identity_signing_key,
        client_id: identity_public_key.clone(),
    };
    register_mocked_identity_key(
        &keys_server,
        identity_public_key.clone(),
        sign_cacao(
            &app_domain,
            &account,
            CacaoAuth::Statement(STATEMENT_THIS_DOMAIN.to_owned()),
            identity_public_key.clone(),
            identity_key_details.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
    )
    .await;
    let vars = get_vars();
    let mut relay_client = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;
    let result = watch_subscriptions(
        &mut relay_client,
        notify_server.url.clone(),
        &identity_key_details,
        Some(app_domain),
        &account,
    )
    .await;
    assert!(result.is_err());

    // Separate limit for no app domain
    futures_util::stream::iter(0..SUBSCRIPTION_WATCHER_LIMIT)
        .map(|_| {
            let keys_server = keys_server.clone();
            let keys_server_url = keys_server_url.clone();
            let account_signing_key = account_signing_key.clone();
            let account = account.clone();
            async move {
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
                        &DidWeb::from_domain("com.example.appbundle".to_owned()),
                        &account,
                        CacaoAuth::Statement(STATEMENT_ALL_DOMAINS.to_owned()),
                        identity_public_key.clone(),
                        identity_key_details.keys_server_url.to_string(),
                        &account_signing_key,
                    )
                    .await,
                )
                .await;
                let vars = get_vars();
                let mut relay_client = RelayClient::new(
                    vars.relay_url.parse().unwrap(),
                    vars.project_id.into(),
                    notify_server.url.clone(),
                )
                .await;
                watch_subscriptions(
                    &mut relay_client,
                    notify_server.url.clone(),
                    &identity_key_details,
                    None,
                    &account,
                )
                .await
            }
        })
        .buffer_unordered(10)
        .collect::<Vec<_>>()
        .await;

    let (identity_signing_key, identity_public_key) = generate_identity_key();
    let identity_key_details = IdentityKeyDetails {
        keys_server_url: keys_server_url.clone(),
        signing_key: identity_signing_key,
        client_id: identity_public_key.clone(),
    };
    register_mocked_identity_key(
        &keys_server,
        identity_public_key.clone(),
        sign_cacao(
            &DidWeb::from_domain("com.example.appbundle".to_owned()),
            &account,
            CacaoAuth::Statement(STATEMENT_THIS_DOMAIN.to_owned()),
            identity_public_key.clone(),
            identity_key_details.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
    )
    .await;
    let vars = get_vars();
    let mut relay_client = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;
    let result = watch_subscriptions(
        &mut relay_client,
        notify_server.url.clone(),
        &identity_key_details,
        None,
        &account,
    )
    .await;
    assert!(result.is_err());
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn notification_link(notify_server: &NotifyServerContext) {
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
    let app_domain = DidWeb::from_domain(format!("{project_id}.example.com"));
    let (key_agreement, authentication, app_client_id) =
        subscribe_topic(&project_id, app_domain.clone(), &notify_server.url).await;

    register_mocked_identity_key(
        &keys_server,
        identity_public_key.clone(),
        sign_cacao(
            &app_domain,
            &account,
            CacaoAuth::Statement(STATEMENT_THIS_DOMAIN.to_owned()),
            identity_public_key.clone(),
            identity_key_details.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
    )
    .await;

    let vars = get_vars();
    let mut relay_client = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    let notification_type = Uuid::new_v4();
    let notification_types = HashSet::from([notification_type]);
    let subs = subscribe_v1(
        &mut relay_client,
        &account,
        &identity_key_details,
        key_agreement,
        &app_client_id,
        app_domain.clone(),
        notification_types.clone(),
    )
    .await;
    assert_eq!(subs.len(), 1);
    let sub = &subs[0];
    assert_eq!(sub.scope, notification_types);

    let notify_key = decode_key(&sub.sym_key).unwrap();
    relay_client.subscribe(topic_from_key(&notify_key)).await;

    let test_url = "https://example.com";

    let notification = Notification {
        r#type: notification_type,
        title: "title".to_owned(),
        body: "body".to_owned(),
        icon: None,
        url: Some(test_url.to_owned()),
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
        &mut relay_client,
        &account,
        &authentication,
        &app_client_id,
        &app_domain,
        &notify_key,
    )
    .await
    .unwrap();

    assert_eq!(notification.r#type, claims.msg.r#type);
    assert_eq!(notification.title, claims.msg.title);
    assert_eq!(notification.body, claims.msg.body);
    assert_ne!(notification.url.as_ref(), Some(&claims.msg.url));

    let result = get_notifications(
        &mut relay_client,
        &account,
        &identity_key_details,
        &app_domain,
        &app_client_id,
        notify_key,
        GetNotificationsParams {
            limit: 5,
            after: None,
            unread_first: false,
        },
    )
    .await
    .unwrap();
    assert_eq!(result.notifications.len(), 1);
    assert!(!result.has_more);

    let gotten_notification = &result.notifications[0];
    assert_eq!(notification.r#type, gotten_notification.r#type);
    assert_eq!(notification.title, gotten_notification.title);
    assert_eq!(notification.body, gotten_notification.body);
    assert_ne!(notification.url, gotten_notification.url);

    assert_eq!(gotten_notification.url.as_ref(), Some(&claims.msg.url));

    let response = reqwest::Client::builder()
        .redirect(redirect::Policy::none())
        .build()
        .unwrap()
        .get(claims.msg.url.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);
    let location_header = response.headers().get("location");
    assert!(location_header.is_some());
    assert_eq!(location_header.unwrap().to_str().unwrap(), test_url);
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn notification_link_no_link(notify_server: &NotifyServerContext) {
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
    let app_domain = DidWeb::from_domain(format!("{project_id}.example.com"));
    let (key_agreement, authentication, app_client_id) =
        subscribe_topic(&project_id, app_domain.clone(), &notify_server.url).await;

    register_mocked_identity_key(
        &keys_server,
        identity_public_key.clone(),
        sign_cacao(
            &app_domain,
            &account,
            CacaoAuth::Statement(STATEMENT_THIS_DOMAIN.to_owned()),
            identity_public_key.clone(),
            identity_key_details.keys_server_url.to_string(),
            &account_signing_key,
        )
        .await,
    )
    .await;

    let vars = get_vars();
    let mut relay_client = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    let notification_type = Uuid::new_v4();
    let notification_types = HashSet::from([notification_type]);
    let subs = subscribe_v1(
        &mut relay_client,
        &account,
        &identity_key_details,
        key_agreement,
        &app_client_id,
        app_domain.clone(),
        notification_types.clone(),
    )
    .await;
    assert_eq!(subs.len(), 1);
    let sub = &subs[0];
    assert_eq!(sub.scope, notification_types);

    let notify_key = decode_key(&sub.sym_key).unwrap();
    relay_client.subscribe(topic_from_key(&notify_key)).await;

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
        &mut relay_client,
        &account,
        &authentication,
        &app_client_id,
        &app_domain,
        &notify_key,
    )
    .await
    .unwrap();

    assert_eq!(notification.r#type, claims.msg.r#type);
    assert_eq!(notification.title, claims.msg.title);
    assert_eq!(notification.body, claims.msg.body);
    assert_eq!("", claims.msg.url);

    let result = get_notifications(
        &mut relay_client,
        &account,
        &identity_key_details,
        &app_domain,
        &app_client_id,
        notify_key,
        GetNotificationsParams {
            limit: 5,
            after: None,
            unread_first: false,
        },
    )
    .await
    .unwrap();
    assert_eq!(result.notifications.len(), 1);
    assert!(!result.has_more);

    let gotten_notification = &result.notifications[0];
    assert_eq!(notification.r#type, gotten_notification.r#type);
    assert_eq!(notification.title, gotten_notification.title);
    assert_eq!(notification.body, gotten_notification.body);
    assert_eq!(notification.url, None);

    assert_eq!(claims.msg.id, gotten_notification.id);

    let response = reqwest::Client::builder()
        .redirect(redirect::Policy::none())
        .build()
        .unwrap()
        .get(format_follow_link(
            &notify_server.url,
            &gotten_notification.id,
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn notification_link_multiple_subscribers_different_links(
    notify_server: &NotifyServerContext,
) {
    let (account_signing_key1, account1) = generate_account();
    let (account_signing_key2, account2) = generate_account();

    let keys_server = MockServer::start().await;
    let keys_server_url = keys_server.uri().parse::<Url>().unwrap();

    let (identity_signing_key1, identity_public_key1) = generate_identity_key();
    let identity_key_details1 = IdentityKeyDetails {
        keys_server_url: keys_server_url.clone(),
        signing_key: identity_signing_key1,
        client_id: identity_public_key1.clone(),
    };
    let (identity_signing_key2, identity_public_key2) = generate_identity_key();
    let identity_key_details2 = IdentityKeyDetails {
        keys_server_url,
        signing_key: identity_signing_key2,
        client_id: identity_public_key2.clone(),
    };

    let project_id = ProjectId::generate();
    let app_domain = DidWeb::from_domain(format!("{project_id}.example.com"));
    let (key_agreement, authentication, app_client_id) =
        subscribe_topic(&project_id, app_domain.clone(), &notify_server.url).await;

    register_mocked_identity_key(
        &keys_server,
        identity_public_key1.clone(),
        sign_cacao(
            &app_domain,
            &account1,
            CacaoAuth::Statement(STATEMENT_THIS_DOMAIN.to_owned()),
            identity_public_key1.clone(),
            identity_key_details1.keys_server_url.to_string(),
            &account_signing_key1,
        )
        .await,
    )
    .await;
    register_mocked_identity_key(
        &keys_server,
        identity_public_key2.clone(),
        sign_cacao(
            &app_domain,
            &account2,
            CacaoAuth::Statement(STATEMENT_THIS_DOMAIN.to_owned()),
            identity_public_key2.clone(),
            identity_key_details2.keys_server_url.to_string(),
            &account_signing_key2,
        )
        .await,
    )
    .await;

    let vars = get_vars();
    let mut relay_client = RelayClient::new(
        vars.relay_url.parse().unwrap(),
        vars.project_id.into(),
        notify_server.url.clone(),
    )
    .await;

    let notification_type = Uuid::new_v4();
    let notification_types = HashSet::from([notification_type]);

    let subs1 = subscribe_v1(
        &mut relay_client,
        &account1,
        &identity_key_details1,
        key_agreement,
        &app_client_id,
        app_domain.clone(),
        notification_types.clone(),
    )
    .await;
    assert_eq!(subs1.len(), 1);
    let sub1 = &subs1[0];
    assert_eq!(sub1.scope, notification_types);
    let notify_key1 = decode_key(&sub1.sym_key).unwrap();
    relay_client.subscribe(topic_from_key(&notify_key1)).await;

    let subs2 = subscribe_v1(
        &mut relay_client,
        &account2,
        &identity_key_details2,
        key_agreement,
        &app_client_id,
        app_domain.clone(),
        notification_types.clone(),
    )
    .await;
    assert_eq!(subs2.len(), 1);
    let sub2 = &subs2[0];
    assert_eq!(sub2.scope, notification_types);
    assert_ne!(sub2.sym_key, sub1.sym_key);
    let notify_key2 = decode_key(&sub2.sym_key).unwrap();
    relay_client.subscribe(topic_from_key(&notify_key2)).await;

    let test_url = "https://example.com";

    let notification = Notification {
        r#type: notification_type,
        title: "title".to_owned(),
        body: "body".to_owned(),
        icon: None,
        url: Some(test_url.to_owned()),
    };

    let notify_body = NotifyBody {
        notification_id: None,
        notification: notification.clone(),
        accounts: vec![account1.clone(), account2.clone()],
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

    let mut relay_client1 = relay_client.clone();
    let (_, claims1) = accept_notify_message(
        &mut relay_client1,
        &account1,
        &authentication,
        &app_client_id,
        &app_domain,
        &notify_key1,
    )
    .await
    .unwrap();

    assert_eq!(notification.r#type, claims1.msg.r#type);
    assert_eq!(notification.title, claims1.msg.title);
    assert_eq!(notification.body, claims1.msg.body);
    assert_ne!(notification.url.as_ref(), Some(&claims1.msg.url));

    let (_, claims2) = accept_notify_message(
        &mut relay_client,
        &account2,
        &authentication,
        &app_client_id,
        &app_domain,
        &notify_key2,
    )
    .await
    .unwrap();

    assert_eq!(notification.r#type, claims2.msg.r#type);
    assert_eq!(notification.title, claims2.msg.title);
    assert_eq!(notification.body, claims2.msg.body);
    assert_ne!(notification.url.as_ref(), Some(&claims2.msg.url));

    let result1 = get_notifications(
        &mut relay_client,
        &account1,
        &identity_key_details1,
        &app_domain,
        &app_client_id,
        notify_key1,
        GetNotificationsParams {
            limit: 5,
            after: None,
            unread_first: false,
        },
    )
    .await
    .unwrap();
    assert_eq!(result1.notifications.len(), 1);
    assert!(!result1.has_more);

    let gotten_notification1 = &result1.notifications[0];
    assert_eq!(notification.r#type, gotten_notification1.r#type);
    assert_eq!(notification.title, gotten_notification1.title);
    assert_eq!(notification.body, gotten_notification1.body);
    assert_ne!(notification.url, gotten_notification1.url);

    assert_eq!(gotten_notification1.url.as_ref(), Some(&claims1.msg.url));

    let result2 = get_notifications(
        &mut relay_client,
        &account2,
        &identity_key_details2,
        &app_domain,
        &app_client_id,
        notify_key2,
        GetNotificationsParams {
            limit: 5,
            after: None,
            unread_first: false,
        },
    )
    .await
    .unwrap();
    assert_eq!(result2.notifications.len(), 1);
    assert!(!result2.has_more);

    let gotten_notification2 = &result2.notifications[0];
    assert_eq!(notification.r#type, gotten_notification2.r#type);
    assert_eq!(notification.title, gotten_notification2.title);
    assert_eq!(notification.body, gotten_notification2.body);
    assert_ne!(notification.url, gotten_notification2.url);

    assert_eq!(gotten_notification2.url.as_ref(), Some(&claims2.msg.url));

    assert_ne!(claims1.msg.id, claims2.msg.id);
    assert_ne!(claims1.msg.url, claims2.msg.url);
    assert_ne!(gotten_notification1.id, gotten_notification2.id);
    assert_ne!(gotten_notification1.url, gotten_notification2.url);

    let response = reqwest::Client::builder()
        .redirect(redirect::Policy::none())
        .build()
        .unwrap()
        .get(claims1.msg.url.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);
    let location_header = response.headers().get("location");
    assert!(location_header.is_some());
    assert_eq!(location_header.unwrap().to_str().unwrap(), test_url);

    let response = reqwest::Client::builder()
        .redirect(redirect::Policy::none())
        .build()
        .unwrap()
        .get(claims2.msg.url.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);
    let location_header = response.headers().get("location");
    assert!(location_header.is_some());
    assert_eq!(location_header.unwrap().to_str().unwrap(), test_url);
}

#[tokio::test]
async fn mark_no_notification_as_read() {
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
    let SubscribeResponse { id: subscriber, .. } = upsert_subscriber(
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

    let results = mark_notifications_as_read(subscriber, None, &postgres, None)
        .await
        .unwrap();
    assert!(results.is_empty());
}

#[tokio::test]
async fn mark_notification_as_read() {
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
    let SubscribeResponse { id: subscriber, .. } = upsert_subscriber(
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
    #[derive(Debug, FromRow)]
    struct Result {
        id: Uuid,
        is_read: bool,
    }
    let Result {
        id: subscriber_notification1,
        is_read,
    } = sqlx::query_as::<_, Result>(
        "SELECT id, is_read FROM subscriber_notification WHERE subscriber=$1 AND notification=$2",
    )
    .bind(subscriber)
    .bind(notification_with_id.id)
    .fetch_one(&postgres)
    .await
    .unwrap();
    assert!(!is_read);

    let results = mark_notifications_as_read(
        subscriber,
        Some(vec![subscriber_notification1]),
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].notification_id, notification_with_id.id);
    assert_eq!(
        results[0].subscriber_notification_id,
        subscriber_notification1
    );

    let results =
        sqlx::query_as::<_, Result>("SELECT id, is_read FROM subscriber_notification WHERE id=$1")
            .bind(subscriber_notification1)
            .fetch_one(&postgres)
            .await
            .unwrap();
    assert_eq!(results.id, subscriber_notification1);
    assert!(results.is_read);
}

#[tokio::test]
async fn mark_only_identified_notification_as_read_postgres() {
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
    let SubscribeResponse { id: subscriber, .. } = upsert_subscriber(
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
    let notification_with_id1 = upsert_notification(
        Uuid::new_v4().to_string(),
        project.id,
        notification.clone(),
        &postgres,
        None,
    )
    .await
    .unwrap();
    upsert_subscriber_notifications(notification_with_id1.id, &[subscriber], &postgres, None)
        .await
        .unwrap();
    #[derive(Debug, FromRow)]
    struct Result {
        id: Uuid,
        is_read: bool,
    }
    let Result {
        id: subscriber_notification1,
        ..
    } = sqlx::query_as::<_, Result>(
        "SELECT id, is_read FROM subscriber_notification WHERE subscriber=$1 AND notification=$2",
    )
    .bind(subscriber)
    .bind(notification_with_id1.id)
    .fetch_one(&postgres)
    .await
    .unwrap();

    let notification = Notification {
        r#type: Uuid::new_v4(),
        title: "title".to_owned(),
        body: "body".to_owned(),
        icon: None,
        url: None,
    };
    let notification_with_id2 = upsert_notification(
        Uuid::new_v4().to_string(),
        project.id,
        notification.clone(),
        &postgres,
        None,
    )
    .await
    .unwrap();
    upsert_subscriber_notifications(notification_with_id2.id, &[subscriber], &postgres, None)
        .await
        .unwrap();
    let Result {
        id: subscriber_notification2,
        ..
    } = sqlx::query_as::<_, Result>(
        "SELECT id, is_read FROM subscriber_notification WHERE subscriber=$1 AND notification=$2",
    )
    .bind(subscriber)
    .bind(notification_with_id2.id)
    .fetch_one(&postgres)
    .await
    .unwrap();

    let results = mark_notifications_as_read(
        subscriber,
        Some(vec![subscriber_notification2]),
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].notification_id, notification_with_id2.id);
    assert_eq!(
        results[0].subscriber_notification_id,
        subscriber_notification2
    );

    let results =
        sqlx::query_as::<_, Result>("SELECT id, is_read FROM subscriber_notification WHERE id=$1")
            .bind(subscriber_notification1)
            .fetch_one(&postgres)
            .await
            .unwrap();
    assert_eq!(results.id, subscriber_notification1);
    assert!(!results.is_read);

    let results =
        sqlx::query_as::<_, Result>("SELECT id, is_read FROM subscriber_notification WHERE id=$1")
            .bind(subscriber_notification2)
            .fetch_one(&postgres)
            .await
            .unwrap();
    assert_eq!(results.id, subscriber_notification2);
    assert!(results.is_read);
}

#[tokio::test]
async fn mark_all_notifications_as_read_postgres() {
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
    let SubscribeResponse { id: subscriber, .. } = upsert_subscriber(
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
    let notification_with_id1 = upsert_notification(
        Uuid::new_v4().to_string(),
        project.id,
        notification.clone(),
        &postgres,
        None,
    )
    .await
    .unwrap();
    upsert_subscriber_notifications(notification_with_id1.id, &[subscriber], &postgres, None)
        .await
        .unwrap();
    #[derive(Debug, FromRow)]
    struct Result {
        id: Uuid,
        is_read: bool,
    }
    let Result {
        id: subscriber_notification1,
        ..
    } = sqlx::query_as::<_, Result>(
        "SELECT id, is_read FROM subscriber_notification WHERE subscriber=$1 AND notification=$2",
    )
    .bind(subscriber)
    .bind(notification_with_id1.id)
    .fetch_one(&postgres)
    .await
    .unwrap();

    let notification = Notification {
        r#type: Uuid::new_v4(),
        title: "title".to_owned(),
        body: "body".to_owned(),
        icon: None,
        url: None,
    };
    let notification_with_id2 = upsert_notification(
        Uuid::new_v4().to_string(),
        project.id,
        notification.clone(),
        &postgres,
        None,
    )
    .await
    .unwrap();
    upsert_subscriber_notifications(notification_with_id2.id, &[subscriber], &postgres, None)
        .await
        .unwrap();
    let Result {
        id: subscriber_notification2,
        ..
    } = sqlx::query_as::<_, Result>(
        "SELECT id, is_read FROM subscriber_notification WHERE subscriber=$1 AND notification=$2",
    )
    .bind(subscriber)
    .bind(notification_with_id2.id)
    .fetch_one(&postgres)
    .await
    .unwrap();

    let results = mark_notifications_as_read(subscriber, None, &postgres, None)
        .await
        .unwrap();
    assert_eq!(results.len(), 2);
    if results[0].notification_id == notification_with_id1.id {
        assert_eq!(
            results[0].subscriber_notification_id,
            subscriber_notification1
        );
        assert_eq!(
            results[0].subscriber_notification_id,
            subscriber_notification1
        );
        assert_eq!(
            results[1].subscriber_notification_id,
            subscriber_notification2
        );
        assert_eq!(
            results[1].subscriber_notification_id,
            subscriber_notification2
        );
    } else {
        assert_eq!(
            results[1].subscriber_notification_id,
            subscriber_notification1
        );
        assert_eq!(
            results[1].subscriber_notification_id,
            subscriber_notification1
        );
        assert_eq!(
            results[2].subscriber_notification_id,
            subscriber_notification2
        );
        assert_eq!(
            results[2].subscriber_notification_id,
            subscriber_notification2
        );
    }

    let results =
        sqlx::query_as::<_, Result>("SELECT id, is_read FROM subscriber_notification WHERE id=$1")
            .bind(subscriber_notification1)
            .fetch_one(&postgres)
            .await
            .unwrap();
    assert_eq!(results.id, subscriber_notification1);
    assert!(results.is_read);

    let results =
        sqlx::query_as::<_, Result>("SELECT id, is_read FROM subscriber_notification WHERE id=$1")
            .bind(subscriber_notification2)
            .fetch_one(&postgres)
            .await
            .unwrap();
    assert_eq!(results.id, subscriber_notification2);
    assert!(results.is_read);
}

#[tokio::test]
async fn mark_single_notification_as_read_only_same_subscriber_postgres() {
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

    let account_id1 = generate_account_id();
    let subscriber_sym_key1 = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let subscriber_topic1 = topic_from_key(&subscriber_sym_key1);
    let subscriber_scope1 = HashSet::from([Uuid::new_v4(), Uuid::new_v4()]);
    let SubscribeResponse {
        id: subscriber1, ..
    } = upsert_subscriber(
        project.id,
        account_id1.clone(),
        subscriber_scope1.clone(),
        &subscriber_sym_key1,
        subscriber_topic1.clone(),
        &postgres,
        None,
    )
    .await
    .unwrap();

    let account_id2 = generate_account_id();
    let subscriber_sym_key2 = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let subscriber_topic2 = topic_from_key(&subscriber_sym_key2);
    let subscriber_scope2 = HashSet::from([Uuid::new_v4(), Uuid::new_v4()]);
    let SubscribeResponse {
        id: subscriber2, ..
    } = upsert_subscriber(
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

    let notification = Notification {
        r#type: Uuid::new_v4(),
        title: "title".to_owned(),
        body: "body".to_owned(),
        icon: None,
        url: None,
    };
    let notification_with_id1 = upsert_notification(
        Uuid::new_v4().to_string(),
        project.id,
        notification.clone(),
        &postgres,
        None,
    )
    .await
    .unwrap();
    upsert_subscriber_notifications(notification_with_id1.id, &[subscriber1], &postgres, None)
        .await
        .unwrap();
    #[derive(Debug, FromRow)]
    struct Result {
        id: Uuid,
        is_read: bool,
    }
    let Result {
        id: subscriber_notification1,
        ..
    } = sqlx::query_as::<_, Result>(
        "SELECT id, is_read FROM subscriber_notification WHERE subscriber=$1 AND notification=$2",
    )
    .bind(subscriber1)
    .bind(notification_with_id1.id)
    .fetch_one(&postgres)
    .await
    .unwrap();

    let results = mark_notifications_as_read(
        subscriber2,
        Some(vec![subscriber_notification1]),
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert!(results.is_empty());

    let results =
        sqlx::query_as::<_, Result>("SELECT id, is_read FROM subscriber_notification WHERE id=$1")
            .bind(subscriber_notification1)
            .fetch_one(&postgres)
            .await
            .unwrap();
    assert_eq!(results.id, subscriber_notification1);
    assert!(!results.is_read);
}

#[tokio::test]
async fn mark_all_notifications_as_read_only_same_subscriber_postgres() {
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

    let account_id1 = generate_account_id();
    let subscriber_sym_key1 = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let subscriber_topic1 = topic_from_key(&subscriber_sym_key1);
    let subscriber_scope1 = HashSet::from([Uuid::new_v4(), Uuid::new_v4()]);
    let SubscribeResponse {
        id: subscriber1, ..
    } = upsert_subscriber(
        project.id,
        account_id1.clone(),
        subscriber_scope1.clone(),
        &subscriber_sym_key1,
        subscriber_topic1.clone(),
        &postgres,
        None,
    )
    .await
    .unwrap();

    let account_id2 = generate_account_id();
    let subscriber_sym_key2 = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let subscriber_topic2 = topic_from_key(&subscriber_sym_key2);
    let subscriber_scope2 = HashSet::from([Uuid::new_v4(), Uuid::new_v4()]);
    let SubscribeResponse {
        id: subscriber2, ..
    } = upsert_subscriber(
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

    let notification = Notification {
        r#type: Uuid::new_v4(),
        title: "title".to_owned(),
        body: "body".to_owned(),
        icon: None,
        url: None,
    };
    let notification_with_id1 = upsert_notification(
        Uuid::new_v4().to_string(),
        project.id,
        notification.clone(),
        &postgres,
        None,
    )
    .await
    .unwrap();
    upsert_subscriber_notifications(notification_with_id1.id, &[subscriber1], &postgres, None)
        .await
        .unwrap();
    #[derive(Debug, FromRow)]
    struct Result {
        id: Uuid,
        is_read: bool,
    }
    let Result {
        id: subscriber_notification1,
        ..
    } = sqlx::query_as::<_, Result>(
        "SELECT id, is_read FROM subscriber_notification WHERE subscriber=$1 AND notification=$2",
    )
    .bind(subscriber1)
    .bind(notification_with_id1.id)
    .fetch_one(&postgres)
    .await
    .unwrap();

    let results = mark_notifications_as_read(subscriber2, None, &postgres, None)
        .await
        .unwrap();
    assert!(results.is_empty());

    let results =
        sqlx::query_as::<_, Result>("SELECT id, is_read FROM subscriber_notification WHERE id=$1")
            .bind(subscriber_notification1)
            .fetch_one(&postgres)
            .await
            .unwrap();
    assert_eq!(results.id, subscriber_notification1);
    assert!(!results.is_read);
}

#[tokio::test]
async fn does_not_mark_already_read_notification_as_read_postgres() {
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
    let SubscribeResponse { id: subscriber, .. } = upsert_subscriber(
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
    #[derive(Debug, FromRow)]
    struct Result {
        id: Uuid,
        is_read: bool,
    }
    let Result {
        id: subscriber_notification,
        ..
    } = sqlx::query_as::<_, Result>(
        "SELECT id, is_read FROM subscriber_notification WHERE subscriber=$1 AND notification=$2",
    )
    .bind(subscriber)
    .bind(notification_with_id.id)
    .fetch_one(&postgres)
    .await
    .unwrap();

    let results = mark_notifications_as_read(
        subscriber,
        Some(vec![subscriber_notification]),
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].notification_id, notification_with_id.id);
    assert_eq!(
        results[0].subscriber_notification_id,
        subscriber_notification
    );

    let results =
        sqlx::query_as::<_, Result>("SELECT id, is_read FROM subscriber_notification WHERE id=$1")
            .bind(subscriber_notification)
            .fetch_one(&postgres)
            .await
            .unwrap();
    assert_eq!(results.id, subscriber_notification);
    assert!(results.is_read);

    let results = mark_notifications_as_read(
        subscriber,
        Some(vec![subscriber_notification]),
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert!(results.is_empty());

    let results =
        sqlx::query_as::<_, Result>("SELECT id, is_read FROM subscriber_notification WHERE id=$1")
            .bind(subscriber_notification)
            .fetch_one(&postgres)
            .await
            .unwrap();
    assert_eq!(results.id, subscriber_notification);
    assert!(results.is_read);
}

#[tokio::test]
async fn get_subscriptions_by_account_and_maybe_app_unread_count_multiple_scopes() {
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
    let SubscribeResponse { id: subscriber, .. } = upsert_subscriber(
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

    let subscribers =
        get_subscriptions_by_account_and_maybe_app(account_id.clone(), None, &postgres, None)
            .await
            .unwrap();
    assert_eq!(subscribers.len(), 1);
    let sub = &subscribers[0];
    assert_eq!(sub.app_domain, project.app_domain);
    assert_eq!(sub.account, account_id);
    assert_eq!(sub.sym_key, hex::encode(subscriber_sym_key));
    assert_eq!(sub.scope, subscriber_scope);
    assert_eq!(sub.unread_notification_count, 0);

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

    let subscribers = get_subscriptions_by_account_and_maybe_app(
        account_id.clone(),
        Some(&app_domain),
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(subscribers.len(), 1);
    let sub = &subscribers[0];
    assert_eq!(sub.app_domain, project.app_domain);
    assert_eq!(sub.account, account_id);
    assert_eq!(sub.sym_key, hex::encode(subscriber_sym_key));
    assert_eq!(sub.scope, subscriber_scope);
    assert_eq!(sub.unread_notification_count, 1);
}

#[tokio::test]
async fn get_subscriptions_by_account_and_maybe_app_unread() {
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
    let SubscribeResponse { id: subscriber, .. } = upsert_subscriber(
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

    let subscribers =
        get_subscriptions_by_account_and_maybe_app(account_id.clone(), None, &postgres, None)
            .await
            .unwrap();
    assert_eq!(subscribers.len(), 1);
    let sub = &subscribers[0];
    assert_eq!(sub.app_domain, project.app_domain);
    assert_eq!(sub.account, account_id);
    assert_eq!(sub.sym_key, hex::encode(subscriber_sym_key));
    assert_eq!(sub.scope, subscriber_scope);
    assert_eq!(sub.unread_notification_count, 0);

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
    #[derive(Debug, FromRow)]
    struct Result {
        id: Uuid,
    }
    let Result {
        id: subscriber_notification,
    } = sqlx::query_as::<_, Result>(
        "SELECT id, is_read FROM subscriber_notification WHERE subscriber=$1 AND notification=$2",
    )
    .bind(subscriber)
    .bind(notification_with_id.id)
    .fetch_one(&postgres)
    .await
    .unwrap();

    let subscribers = get_subscriptions_by_account_and_maybe_app(
        account_id.clone(),
        Some(&app_domain),
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(subscribers.len(), 1);
    let sub = &subscribers[0];
    assert_eq!(sub.app_domain, project.app_domain);
    assert_eq!(sub.account, account_id);
    assert_eq!(sub.sym_key, hex::encode(subscriber_sym_key));
    assert_eq!(sub.scope, subscriber_scope);
    assert_eq!(sub.unread_notification_count, 1);

    let subscribers =
        get_subscriptions_by_account_and_maybe_app(account_id.clone(), None, &postgres, None)
            .await
            .unwrap();
    assert_eq!(subscribers.len(), 1);
    let sub = &subscribers[0];
    assert_eq!(sub.app_domain, project.app_domain);
    assert_eq!(sub.account, account_id);
    assert_eq!(sub.sym_key, hex::encode(subscriber_sym_key));
    assert_eq!(sub.scope, subscriber_scope);
    assert_eq!(sub.unread_notification_count, 1);

    let results = mark_notifications_as_read(
        subscriber,
        Some(vec![subscriber_notification]),
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].notification_id, notification_with_id.id);
    assert_eq!(
        results[0].subscriber_notification_id,
        subscriber_notification
    );

    let subscribers =
        get_subscriptions_by_account_and_maybe_app(account_id.clone(), None, &postgres, None)
            .await
            .unwrap();
    assert_eq!(subscribers.len(), 1);
    let sub = &subscribers[0];
    assert_eq!(sub.app_domain, project.app_domain);
    assert_eq!(sub.account, account_id);
    assert_eq!(sub.sym_key, hex::encode(subscriber_sym_key));
    assert_eq!(sub.scope, subscriber_scope);
    assert_eq!(sub.unread_notification_count, 0);
}

#[test_context(NotifyServerContext)]
#[tokio::test]
async fn e2e_mark_notification_as_read(notify_server: &NotifyServerContext) {
    let notification_type = Uuid::new_v4();
    let (
        mut relay_client,
        account,
        identity_key_details,
        project_id,
        app_domain,
        app_authentication_key,
        app_client_id,
        notify_key,
    ) = setup_subscription(
        notify_server.url.clone(),
        HashSet::from([notification_type]),
    )
    .await;

    let (subs, _watch_topic_key, _notify_server_client_id) = watch_subscriptions(
        &mut relay_client,
        notify_server.url.clone(),
        &identity_key_details,
        Some(app_domain.clone()),
        &account,
    )
    .await
    .unwrap();
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0].unread_notification_count, 0);

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
        &mut relay_client,
        &account,
        &app_authentication_key,
        &app_client_id,
        &app_domain,
        &notify_key,
    )
    .await
    .unwrap();
    assert_eq!(claims.msg.r#type, notification.r#type);
    assert!(!claims.msg.is_read);

    let result = get_notifications(
        &mut relay_client,
        &account,
        &identity_key_details,
        &app_domain,
        &app_client_id,
        notify_key,
        GetNotificationsParams {
            limit: 5,
            after: None,
            unread_first: false,
        },
    )
    .await
    .unwrap();
    assert_eq!(result.notifications.len(), 1);
    assert_eq!(result.notifications[0].r#type, notification.r#type);
    assert!(!result.notifications[0].is_read);

    let (subs, _watch_topic_key, _notify_server_client_id) = watch_subscriptions(
        &mut relay_client,
        notify_server.url.clone(),
        &identity_key_details,
        Some(app_domain.clone()),
        &account,
    )
    .await
    .unwrap();
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0].unread_notification_count, 1);

    helper_mark_notifications_as_read(
        &mut relay_client,
        &account,
        &identity_key_details,
        &app_domain,
        &app_client_id,
        notify_key,
        MarkNotificationsAsReadParams {
            all: false,
            ids: Some(vec![result.notifications[0].id]),
        },
    )
    .await
    .unwrap();

    let result = get_notifications(
        &mut relay_client,
        &account,
        &identity_key_details,
        &app_domain,
        &app_client_id,
        notify_key,
        GetNotificationsParams {
            limit: 5,
            after: None,
            unread_first: false,
        },
    )
    .await
    .unwrap();
    assert_eq!(result.notifications.len(), 1);
    assert_eq!(result.notifications[0].r#type, notification.r#type);
    assert!(result.notifications[0].is_read);

    let (subs, _watch_topic_key, _notify_server_client_id) = watch_subscriptions(
        &mut relay_client,
        notify_server.url.clone(),
        &identity_key_details,
        Some(app_domain.clone()),
        &account,
    )
    .await
    .unwrap();
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0].unread_notification_count, 0);
}

async fn helper_get_notifications() -> (Uuid, Uuid, PgPool) {
    let (postgres, _) = get_postgres().await;
    let results = helper_get_notifications_with_postgres(&postgres).await;
    (results.0, results.1, postgres)
}

async fn helper_get_notifications_with_postgres(postgres: &PgPool) -> (Uuid, Uuid) {
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
        postgres,
        None,
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), postgres, None)
        .await
        .unwrap();

    let account_id = generate_account_id();
    let subscriber_sym_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let subscriber_topic = topic_from_key(&subscriber_sym_key);
    let subscriber_scope = HashSet::from([Uuid::new_v4(), Uuid::new_v4()]);
    let SubscribeResponse { id: subscriber, .. } = upsert_subscriber(
        project.id,
        account_id.clone(),
        subscriber_scope.clone(),
        &subscriber_sym_key,
        subscriber_topic.clone(),
        postgres,
        None,
    )
    .await
    .unwrap();

    (project.id, subscriber)
}

async fn helper_insert_notifications(
    notifications: &[(Uuid, DateTime<Utc>, bool)],
    project: Uuid,
    subscriber: Uuid,
    postgres: &PgPool,
) {
    for (subscriber_notification_id, created_at, is_read) in notifications.iter() {
        #[derive(Debug, FromRow)]
        struct NotificationResult {
            id: Uuid,
        }
        let NotificationResult {
            id: notification_id,
        } = sqlx::query_as::<Postgres, NotificationResult>(
            "
            INSERT INTO notification (project, notification_id, type, title, body)
            VALUES ($1, $2, $3, '', '')
            RETURNING id
            ",
        )
        .bind(project)
        .bind(Uuid::new_v4())
        .bind(Uuid::new_v4())
        .fetch_one(postgres)
        .await
        .unwrap();

        sqlx::query(
            "
            INSERT INTO subscriber_notification (id, created_at, subscriber, notification, is_read, status)
            VALUES ($1, $2, $3, $4, $5, 'queued'::subscriber_notification_status)
            ",
        )
        .bind(subscriber_notification_id)
        .bind(created_at)
        .bind(subscriber)
        .bind(notification_id)
        .bind(is_read)
        .execute(postgres)
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn test_get_notifications_empty() {
    let (_project, subscriber, postgres) = helper_get_notifications().await;

    let result = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams {
            limit: 1,
            after: None,
            unread_first: false,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert!(result.notifications.is_empty());
    assert!(!result.has_more);
    assert!(!result.has_more_unread);

    let result = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams {
            limit: 1,
            after: None,
            unread_first: true,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert!(result.notifications.is_empty());
    assert!(!result.has_more);
    assert!(!result.has_more_unread);
}

#[tokio::test]
async fn test_get_notifications_single_unread() {
    let (project, subscriber, postgres) = helper_get_notifications().await;
    let notification1 = Uuid::from_bytes([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    helper_insert_notifications(
        &[(
            notification1,
            DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
            false,
        )],
        project,
        subscriber,
        &postgres,
    )
    .await;

    let result = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams {
            limit: 1,
            after: None,
            unread_first: false,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.notifications.len(), 1);
    assert!(!result.has_more);
    assert!(!result.has_more_unread);
    assert_eq!(result.notifications[0].id, notification1);
    assert!(!result.notifications[0].is_read);

    let result = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams {
            limit: 1,
            after: None,
            unread_first: true,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.notifications.len(), 1);
    assert!(!result.has_more);
    assert!(!result.has_more_unread);
    assert_eq!(result.notifications[0].id, notification1);
    assert!(!result.notifications[0].is_read);
}

#[tokio::test]
async fn test_get_notifications_two_unread() {
    let (project, subscriber, postgres) = helper_get_notifications().await;
    let notification1 = Uuid::from_bytes([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    let notification2 = Uuid::from_bytes([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    helper_insert_notifications(
        &[
            (
                notification1,
                DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
                false,
            ),
            (
                notification2,
                DateTime::<Utc>::from_timestamp(1, 0).unwrap(),
                false,
            ),
        ],
        project,
        subscriber,
        &postgres,
    )
    .await;

    let result = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams {
            limit: 2,
            after: None,
            unread_first: false,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.notifications.len(), 2);
    assert!(!result.has_more);
    assert!(!result.has_more_unread);
    assert_eq!(result.notifications[0].id, notification2);
    assert!(!result.notifications[0].is_read);
    assert_eq!(result.notifications[1].id, notification1);
    assert!(!result.notifications[1].is_read);

    let result = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams {
            limit: 2,
            after: None,
            unread_first: true,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.notifications.len(), 2);
    assert!(!result.has_more);
    assert!(!result.has_more_unread);
    assert_eq!(result.notifications[0].id, notification2);
    assert!(!result.notifications[0].is_read);
    assert_eq!(result.notifications[1].id, notification1);
    assert!(!result.notifications[1].is_read);
}

#[tokio::test]
async fn test_get_notifications_two_unread_same_created() {
    let (project, subscriber, postgres) = helper_get_notifications().await;
    let notification1 = Uuid::from_bytes([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let notification2 = Uuid::from_bytes([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    helper_insert_notifications(
        &[
            (
                notification1,
                DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
                false,
            ),
            (
                notification2,
                DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
                false,
            ),
        ],
        project,
        subscriber,
        &postgres,
    )
    .await;

    let result = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams {
            limit: 2,
            after: None,
            unread_first: false,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.notifications.len(), 2);
    assert!(!result.has_more);
    assert!(!result.has_more_unread);
    assert_eq!(result.notifications[0].id, notification1);
    assert!(!result.notifications[0].is_read);
    assert_eq!(result.notifications[1].id, notification2);
    assert!(!result.notifications[1].is_read);

    let result = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams {
            limit: 2,
            after: None,
            unread_first: true,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.notifications.len(), 2);
    assert!(!result.has_more);
    assert!(!result.has_more_unread);
    assert_eq!(result.notifications[0].id, notification1);
    assert!(!result.notifications[0].is_read);
    assert_eq!(result.notifications[1].id, notification2);
    assert!(!result.notifications[1].is_read);
}

#[tokio::test]
async fn test_get_notifications_after_two_unread() {
    let (project, subscriber, postgres) = helper_get_notifications().await;
    let notification1 = Uuid::from_bytes([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    let notification2 = Uuid::from_bytes([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    helper_insert_notifications(
        &[
            (
                notification1,
                DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
                false,
            ),
            (
                notification2,
                DateTime::<Utc>::from_timestamp(1, 0).unwrap(),
                false,
            ),
        ],
        project,
        subscriber,
        &postgres,
    )
    .await;

    let result = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams {
            limit: 2,
            after: Some(notification2),
            unread_first: false,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.notifications.len(), 1);
    assert!(!result.has_more);
    assert!(!result.has_more_unread);
    assert_eq!(result.notifications[0].id, notification1);
    assert!(!result.notifications[0].is_read);

    let result = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams {
            limit: 2,
            after: Some(notification2),
            unread_first: true,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.notifications.len(), 1);
    assert!(!result.has_more);
    assert!(!result.has_more_unread);
    assert_eq!(result.notifications[0].id, notification1);
    assert!(!result.notifications[0].is_read);
}

#[tokio::test]
async fn test_get_notifications_after_two_unread_same_created() {
    let (project, subscriber, postgres) = helper_get_notifications().await;
    let notification1 = Uuid::from_bytes([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let notification2 = Uuid::from_bytes([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    helper_insert_notifications(
        &[
            (
                notification1,
                DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
                false,
            ),
            (
                notification2,
                DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
                false,
            ),
        ],
        project,
        subscriber,
        &postgres,
    )
    .await;

    let result = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams {
            limit: 2,
            after: Some(notification1),
            unread_first: false,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.notifications.len(), 1);
    assert!(!result.has_more);
    assert!(!result.has_more_unread);
    assert_eq!(result.notifications[0].id, notification2);
    assert!(!result.notifications[0].is_read);

    let result = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams {
            limit: 2,
            after: Some(notification1),
            unread_first: true,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.notifications.len(), 1);
    assert!(!result.has_more);
    assert!(!result.has_more_unread);
    assert_eq!(result.notifications[0].id, notification2);
    assert!(!result.notifications[0].is_read);
}

#[tokio::test]
async fn test_get_notifications_single_read() {
    let (project, subscriber, postgres) = helper_get_notifications().await;
    let notification1 = Uuid::from_bytes([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    helper_insert_notifications(
        &[(
            notification1,
            DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
            true,
        )],
        project,
        subscriber,
        &postgres,
    )
    .await;

    let result = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams {
            limit: 1,
            after: None,
            unread_first: false,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.notifications.len(), 1);
    assert!(!result.has_more);
    assert!(!result.has_more_unread);
    assert_eq!(result.notifications[0].id, notification1);
    assert!(result.notifications[0].is_read);

    let result = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams {
            limit: 1,
            after: None,
            unread_first: true,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.notifications.len(), 1);
    assert!(!result.has_more);
    assert!(!result.has_more_unread);
    assert_eq!(result.notifications[0].id, notification1);
    assert!(result.notifications[0].is_read);
}

#[tokio::test]
async fn test_get_notifications_two_read() {
    let (project, subscriber, postgres) = helper_get_notifications().await;
    let notification1 = Uuid::from_bytes([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    let notification2 = Uuid::from_bytes([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    helper_insert_notifications(
        &[
            (
                notification1,
                DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
                true,
            ),
            (
                notification2,
                DateTime::<Utc>::from_timestamp(1, 0).unwrap(),
                true,
            ),
        ],
        project,
        subscriber,
        &postgres,
    )
    .await;

    let result = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams {
            limit: 2,
            after: None,
            unread_first: false,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.notifications.len(), 2);
    assert!(!result.has_more);
    assert!(!result.has_more_unread);
    assert_eq!(result.notifications[0].id, notification2);
    assert!(result.notifications[0].is_read);
    assert_eq!(result.notifications[1].id, notification1);
    assert!(result.notifications[1].is_read);

    let result = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams {
            limit: 2,
            after: None,
            unread_first: true,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.notifications.len(), 2);
    assert!(!result.has_more);
    assert!(!result.has_more_unread);
    assert_eq!(result.notifications[0].id, notification2);
    assert!(result.notifications[0].is_read);
    assert_eq!(result.notifications[1].id, notification1);
    assert!(result.notifications[1].is_read);
}

#[tokio::test]
async fn test_get_notifications_read_and_unread() {
    let (project, subscriber, postgres) = helper_get_notifications().await;
    let notification1 = Uuid::from_bytes([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    let notification2 = Uuid::from_bytes([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    helper_insert_notifications(
        &[
            (
                notification1,
                DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
                false,
            ),
            (
                notification2,
                DateTime::<Utc>::from_timestamp(1, 0).unwrap(),
                true,
            ),
        ],
        project,
        subscriber,
        &postgres,
    )
    .await;

    let result = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams {
            limit: 2,
            after: None,
            unread_first: false,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.notifications.len(), 2);
    assert!(!result.has_more);
    assert!(!result.has_more_unread);
    assert_eq!(result.notifications[0].id, notification2);
    assert!(result.notifications[0].is_read);
    assert_eq!(result.notifications[1].id, notification1);
    assert!(!result.notifications[1].is_read);

    let result = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams {
            limit: 2,
            after: None,
            unread_first: true,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.notifications.len(), 2);
    assert!(!result.has_more);
    assert!(!result.has_more_unread);
    assert_eq!(result.notifications[0].id, notification1);
    assert!(!result.notifications[0].is_read);
    assert_eq!(result.notifications[1].id, notification2);
    assert!(result.notifications[1].is_read);
}

#[tokio::test]
async fn test_mark_all_notifications_as_read_for_project_zero_notifications() {
    let (project, subscriber, postgres) = helper_get_notifications().await;
    helper_insert_notifications(&[], project, subscriber, &postgres).await;

    mark_all_notifications_as_read_for_project(project, &postgres, None)
        .await
        .unwrap();

    let result = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams {
            limit: 2,
            after: None,
            unread_first: false,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert!(result.notifications.is_empty());
    assert!(!result.has_more);
    assert!(!result.has_more_unread);
}

#[tokio::test]
async fn test_mark_all_notifications_as_read_for_project_one_notification() {
    let (project, subscriber, postgres) = helper_get_notifications().await;
    let notification1 = Uuid::from_bytes([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    helper_insert_notifications(
        &[(
            notification1,
            DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
            false,
        )],
        project,
        subscriber,
        &postgres,
    )
    .await;

    mark_all_notifications_as_read_for_project(project, &postgres, None)
        .await
        .unwrap();

    let result = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams {
            limit: 2,
            after: None,
            unread_first: false,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.notifications.len(), 1);
    assert!(!result.has_more);
    assert!(!result.has_more_unread);
    assert_eq!(result.notifications[0].id, notification1);
    assert!(!result.notifications[0].is_read);
}

#[tokio::test]
async fn test_mark_all_notifications_as_read_for_project_two_notifications() {
    let (project, subscriber, postgres) = helper_get_notifications().await;
    let notification1 = Uuid::from_bytes([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    let notification2 = Uuid::from_bytes([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    helper_insert_notifications(
        &[
            (
                notification1,
                DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
                false,
            ),
            (
                notification2,
                DateTime::<Utc>::from_timestamp(1, 0).unwrap(),
                false,
            ),
        ],
        project,
        subscriber,
        &postgres,
    )
    .await;

    mark_all_notifications_as_read_for_project(project, &postgres, None)
        .await
        .unwrap();

    let result = get_notifications_for_subscriber(
        subscriber,
        GetNotificationsParams {
            limit: 2,
            after: None,
            unread_first: false,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.notifications.len(), 2);
    assert!(!result.has_more);
    assert!(!result.has_more_unread);
    assert_eq!(result.notifications[0].id, notification2);
    assert!(!result.notifications[0].is_read);
    assert_eq!(result.notifications[1].id, notification1);
    assert!(!result.notifications[1].is_read);
}

#[tokio::test]
async fn test_mark_all_notifications_as_read_for_project_one_notification_this_project_only() {
    let (project1, subscriber1, postgres) = helper_get_notifications().await;
    let (project2, subscriber2) = helper_get_notifications_with_postgres(&postgres).await;
    let notification1 = Uuid::from_bytes([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    helper_insert_notifications(
        &[(
            notification1,
            DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
            false,
        )],
        project1,
        subscriber1,
        &postgres,
    )
    .await;
    let notification2 = Uuid::from_bytes([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    helper_insert_notifications(
        &[(
            notification2,
            DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
            false,
        )],
        project2,
        subscriber2,
        &postgres,
    )
    .await;

    mark_all_notifications_as_read_for_project(project1, &postgres, None)
        .await
        .unwrap();

    let result = get_notifications_for_subscriber(
        subscriber1,
        GetNotificationsParams {
            limit: 2,
            after: None,
            unread_first: false,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.notifications.len(), 1);
    assert!(!result.has_more);
    assert!(!result.has_more_unread);
    assert_eq!(result.notifications[0].id, notification1);
    assert!(!result.notifications[0].is_read);

    let result = get_notifications_for_subscriber(
        subscriber2,
        GetNotificationsParams {
            limit: 2,
            after: None,
            unread_first: false,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.notifications.len(), 1);
    assert!(!result.has_more);
    assert!(!result.has_more_unread);
    assert_eq!(result.notifications[0].id, notification2);
    assert!(!result.notifications[0].is_read);
}

#[tokio::test]
async fn test_mark_all_notifications_as_read_for_multiple_subscribers() {
    let (project, subscriber1, postgres) = helper_get_notifications().await;

    let account_id = generate_account_id();
    let subscriber_sym_key = rand::Rng::gen::<[u8; 32]>(&mut rand::thread_rng());
    let subscriber_topic = topic_from_key(&subscriber_sym_key);
    let subscriber_scope = HashSet::from([Uuid::new_v4(), Uuid::new_v4()]);
    let SubscribeResponse {
        id: subscriber2, ..
    } = upsert_subscriber(
        project,
        account_id.clone(),
        subscriber_scope.clone(),
        &subscriber_sym_key,
        subscriber_topic.clone(),
        &postgres,
        None,
    )
    .await
    .unwrap();

    let notification1 = Uuid::from_bytes([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    helper_insert_notifications(
        &[(
            notification1,
            DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
            false,
        )],
        project,
        subscriber1,
        &postgres,
    )
    .await;
    let notification2 = Uuid::from_bytes([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    helper_insert_notifications(
        &[(
            notification2,
            DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
            false,
        )],
        project,
        subscriber2,
        &postgres,
    )
    .await;

    mark_all_notifications_as_read_for_project(project, &postgres, None)
        .await
        .unwrap();

    let result = get_notifications_for_subscriber(
        subscriber1,
        GetNotificationsParams {
            limit: 2,
            after: None,
            unread_first: false,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.notifications.len(), 1);
    assert!(!result.has_more);
    assert!(!result.has_more_unread);
    assert_eq!(result.notifications[0].id, notification1);
    assert!(!result.notifications[0].is_read);

    let result = get_notifications_for_subscriber(
        subscriber2,
        GetNotificationsParams {
            limit: 2,
            after: None,
            unread_first: false,
        },
        &postgres,
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.notifications.len(), 1);
    assert!(!result.has_more);
    assert!(!result.has_more_unread);
    assert_eq!(result.notifications[0].id, notification2);
    assert!(!result.notifications[0].is_read);
}

// TODO test is_same_address
