use {
    crate::utils::{verify_jwt, JWT_LEEWAY},
    async_trait::async_trait,
    base64::Engine,
    chacha20poly1305::{aead::Aead, ChaCha20Poly1305, KeyInit},
    chrono::{Duration, Utc},
    hyper::StatusCode,
    notify_server::{
        auth::{
            encode_authentication_private_key, encode_authentication_public_key,
            encode_subscribe_private_key, encode_subscribe_public_key,
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
        registry::RegistryAuthResponse,
        services::{
            public_http_server::handlers::{
                notify_v0::NotifyBody, notify_v1::NotifyBodyNotification,
            },
            publisher_service::helpers::{
                dead_letter_give_up_check, dead_letters_check,
                pick_subscriber_notification_for_processing, upsert_notification,
                upsert_subscriber_notifications,
            },
            websocket_server::{relay_ws_client::RelayClientEvent, NotifyRequest},
        },
        spec::NOTIFY_MESSAGE_TAG,
        types::{Envelope, EnvelopeType0, Notification},
    },
    rand_chacha::rand_core::OsRng,
    relay_rpc::domain::{ProjectId, Topic},
    reqwest::Response,
    serde_json::{json, Value},
    sha2::digest::generic_array::GenericArray,
    sqlx::{postgres::PgPoolOptions, PgPool, Postgres},
    std::{
        collections::HashSet,
        env,
        net::{IpAddr, Ipv4Addr, SocketAddr},
    },
    test_context::{test_context, AsyncTestContext},
    tokio::{
        net::{TcpListener, ToSocketAddrs},
        sync::broadcast,
        time::error::Elapsed,
    },
    tracing_subscriber::fmt::format::FmtSpan,
    url::Url,
    utils::create_client,
    uuid::Uuid,
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
        postgres_url: "postgres://postgres:password@localhost:5432/postgres".to_owned(),
    }
}

struct Vars {
    project_id: String,
    relay_url: String,
    postgres_url: String,
}

async fn get_postgres() -> PgPool {
    let postgres = PgPoolOptions::new()
        .connect(&get_vars().postgres_url)
        .await
        .unwrap();
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
    postgres
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
    "eip155:1:0xfff".into()
}

#[tokio::test]
async fn test_one_project() {
    let postgres = get_postgres().await;

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
    )
    .await
    .unwrap();

    assert_eq!(get_subscriber_topics(&postgres).await.unwrap(), vec![]);
    assert_eq!(
        get_project_topics(&postgres).await.unwrap(),
        vec![topic.clone()]
    );
    let project = get_project_by_app_domain(&app_domain, &postgres)
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

    let project = get_project_by_project_id(project_id.clone(), &postgres)
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

    let project = get_project_by_topic(topic.clone(), &postgres)
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
    let postgres = get_postgres().await;

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
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &postgres)
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
    )
    .await
    .unwrap();
    // let subscriber_scope = subscriber_scope.map(|s| s.to_string()).collect::<HashSet<_>>();

    assert_eq!(
        get_subscriber_topics(&postgres).await.unwrap(),
        vec![subscriber_topic.clone()]
    );

    let subscriber = get_subscriber_by_topic(subscriber_topic.clone(), &postgres)
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

    let subscribers = get_subscribers_for_project_in(project.id, &[account_id.clone()], &postgres)
        .await
        .unwrap();
    assert_eq!(subscribers.len(), 1);
    let subscriber = &subscribers[0];
    assert_eq!(subscriber.project, project.id);
    assert_eq!(subscriber.account, account_id);
    assert_eq!(subscriber.sym_key, hex::encode(subscriber_sym_key));
    assert_eq!(subscriber.topic, subscriber_topic);
    assert_eq!(
        subscriber.scope.iter().cloned().collect::<HashSet<_>>(),
        subscriber_scope
    );
    assert!(subscriber.expiry > Utc::now() + Duration::days(29));

    let accounts = get_subscriber_accounts_by_project_id(project_id.clone(), &postgres)
        .await
        .unwrap();
    assert_eq!(accounts, vec![account_id.clone()]);

    let subscribers = get_subscriptions_by_account(account_id.clone(), &postgres)
        .await
        .unwrap();
    assert_eq!(subscribers.len(), 1);
    let subscriber = &subscribers[0];
    assert_eq!(subscriber.app_domain, project.app_domain);
    assert_eq!(subscriber.account, account_id);
    assert_eq!(subscriber.sym_key, hex::encode(subscriber_sym_key));
    assert_eq!(
        subscriber.scope.iter().cloned().collect::<HashSet<_>>(),
        subscriber_scope
    );
    assert!(subscriber.expiry > Utc::now() + Duration::days(29));
}

#[tokio::test]
async fn test_two_subscribers() {
    let postgres = get_postgres().await;

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
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &postgres)
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
    )
    .await
    .unwrap();

    let project = get_project_by_project_id(project_id.clone(), &postgres)
        .await
        .unwrap();

    assert_eq!(
        get_subscriber_topics(&postgres)
            .await
            .unwrap()
            .into_iter()
            .collect::<HashSet<_>>(),
        HashSet::from([subscriber_topic.clone(), subscriber_topic2.clone()])
    );

    let subscriber = get_subscriber_by_topic(subscriber_topic.clone(), &postgres)
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

    let subscriber = get_subscriber_by_topic(subscriber_topic2.clone(), &postgres)
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
    )
    .await
    .unwrap();
    assert_eq!(subscribers.len(), 2);
    for subscriber in subscribers {
        if subscriber.account == account_id {
            assert_eq!(subscriber.project, project.id);
            assert_eq!(subscriber.sym_key, hex::encode(subscriber_sym_key));
            assert_eq!(subscriber.topic, subscriber_topic);
            assert_eq!(
                subscriber.scope.iter().cloned().collect::<HashSet<_>>(),
                subscriber_scope
            );
            assert!(subscriber.expiry > Utc::now() + Duration::days(29));
        } else {
            assert_eq!(subscriber.project, project.id);
            assert_eq!(subscriber.sym_key, hex::encode(subscriber_sym_key2));
            assert_eq!(subscriber.topic, subscriber_topic2);
            assert_eq!(
                subscriber.scope.iter().cloned().collect::<HashSet<_>>(),
                subscriber_scope2
            );
            assert!(subscriber.expiry > Utc::now() + Duration::days(29));
        }
    }

    let accounts = get_subscriber_accounts_by_project_id(project_id.clone(), &postgres)
        .await
        .unwrap();
    assert_eq!(
        accounts.iter().cloned().collect::<HashSet<_>>(),
        HashSet::from([account_id.clone(), account_id2.clone()])
    );

    let subscribers = get_subscriptions_by_account(account_id.clone(), &postgres)
        .await
        .unwrap();
    assert_eq!(subscribers.len(), 1);
    let subscriber = &subscribers[0];
    assert_eq!(subscriber.app_domain, project.app_domain);
    assert_eq!(subscriber.account, account_id);
    assert_eq!(subscriber.sym_key, hex::encode(subscriber_sym_key));
    assert_eq!(
        subscriber.scope.iter().cloned().collect::<HashSet<_>>(),
        subscriber_scope
    );
    assert!(subscriber.expiry > Utc::now() + Duration::days(29));

    let subscribers = get_subscriptions_by_account(account_id2.clone(), &postgres)
        .await
        .unwrap();
    assert_eq!(subscribers.len(), 1);
    let subscriber = &subscribers[0];
    assert_eq!(subscriber.app_domain, project.app_domain);
    assert_eq!(subscriber.account, account_id2);
    assert_eq!(subscriber.sym_key, hex::encode(subscriber_sym_key2));
    assert_eq!(
        subscriber.scope.iter().cloned().collect::<HashSet<_>>(),
        subscriber_scope2
    );
    assert!(subscriber.expiry > Utc::now() + Duration::days(29));
}

#[tokio::test]
async fn test_one_subscriber_two_projects() {
    let postgres = get_postgres().await;

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
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &postgres)
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
    )
    .await
    .unwrap();
    let project2 = get_project_by_project_id(project_id2.clone(), &postgres)
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
    )
    .await
    .unwrap();

    let project = get_project_by_project_id(project_id.clone(), &postgres)
        .await
        .unwrap();
    let project2 = get_project_by_project_id(project_id2.clone(), &postgres)
        .await
        .unwrap();

    assert_eq!(
        get_subscriber_topics(&postgres)
            .await
            .unwrap()
            .into_iter()
            .collect::<HashSet<_>>(),
        HashSet::from([subscriber_topic.clone(), subscriber_topic2.clone()])
    );

    let subscriber = get_subscriber_by_topic(subscriber_topic.clone(), &postgres)
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

    let subscriber = get_subscriber_by_topic(subscriber_topic2.clone(), &postgres)
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

    let subscribers = get_subscribers_for_project_in(project.id, &[account_id.clone()], &postgres)
        .await
        .unwrap();
    assert_eq!(subscribers.len(), 1);
    let subscriber = &subscribers[0];
    assert_eq!(subscriber.project, project.id);
    assert_eq!(subscriber.account, account_id);
    assert_eq!(subscriber.sym_key, hex::encode(subscriber_sym_key));
    assert_eq!(subscriber.topic, subscriber_topic);
    assert_eq!(
        subscriber.scope.iter().cloned().collect::<HashSet<_>>(),
        subscriber_scope
    );
    assert!(subscriber.expiry > Utc::now() + Duration::days(29));

    let subscribers = get_subscribers_for_project_in(project2.id, &[account_id.clone()], &postgres)
        .await
        .unwrap();
    assert_eq!(subscribers.len(), 1);
    let subscriber = &subscribers[0];
    assert_eq!(subscriber.project, project2.id);
    assert_eq!(subscriber.account, account_id);
    assert_eq!(subscriber.sym_key, hex::encode(subscriber_sym_key2));
    assert_eq!(subscriber.topic, subscriber_topic2);
    assert_eq!(
        subscriber.scope.iter().cloned().collect::<HashSet<_>>(),
        subscriber_scope2
    );
    assert!(subscriber.expiry > Utc::now() + Duration::days(29));

    let accounts = get_subscriber_accounts_by_project_id(project_id.clone(), &postgres)
        .await
        .unwrap();
    assert_eq!(accounts, vec![account_id.clone()]);
    let accounts = get_subscriber_accounts_by_project_id(project_id2.clone(), &postgres)
        .await
        .unwrap();
    assert_eq!(accounts, vec![account_id.clone()]);

    let subscribers = get_subscriptions_by_account(account_id.clone(), &postgres)
        .await
        .unwrap();
    assert_eq!(subscribers.len(), 2);
    for subscriber in subscribers {
        if subscriber.app_domain == app_domain {
            assert_eq!(subscriber.app_domain, app_domain);
            assert_eq!(subscriber.account, account_id);
            assert_eq!(subscriber.sym_key, hex::encode(subscriber_sym_key));
            assert_eq!(
                subscriber.scope.iter().cloned().collect::<HashSet<_>>(),
                subscriber_scope
            );
            assert!(subscriber.expiry > Utc::now() + Duration::days(29));
        } else {
            assert_eq!(subscriber.app_domain, app_domain2);
            assert_eq!(subscriber.account, account_id);
            assert_eq!(subscriber.sym_key, hex::encode(subscriber_sym_key2));
            assert_eq!(
                subscriber.scope.iter().cloned().collect::<HashSet<_>>(),
                subscriber_scope2
            );
            assert!(subscriber.expiry > Utc::now() + Duration::days(29));
        }
    }
}

#[tokio::test]
async fn test_account_case_insensitive() {
    let postgres = get_postgres().await;

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
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &postgres)
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
    )
    .await
    .unwrap();

    let subscribers = get_subscriptions_by_account(format!("{addr_prefix}FFF").into(), &postgres)
        .await
        .unwrap();
    assert_eq!(subscribers.len(), 1);
}

#[tokio::test]
async fn test_get_subscriber_accounts_by_project_id() {
    let postgres = get_postgres().await;

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
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &postgres)
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
    )
    .await
    .unwrap();

    let accounts = get_subscriber_accounts_by_project_id(project_id, &postgres)
        .await
        .unwrap();
    assert_eq!(accounts, vec![account]);
}

#[tokio::test]
async fn test_get_subscriber_accounts_and_scopes_by_project_id() {
    let postgres = get_postgres().await;

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
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &postgres)
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
    )
    .await
    .unwrap();

    let subscribers = get_subscriber_accounts_and_scopes_by_project_id(project_id, &postgres)
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
    static NEXT_PORT: AtomicU16 = AtomicU16::new(9000);
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
}

#[async_trait]
impl AsyncTestContext for NotifyServerContext {
    async fn setup() -> Self {
        let mock_server = {
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
        let socket_addr = SocketAddr::from((bind_ip, bind_port));
        let notify_url = format!("http://{socket_addr}").parse::<Url>().unwrap();
        // TODO reuse the local configuration defaults here
        let config = Configuration {
            postgres_url: vars.postgres_url,
            postgres_max_connections: 10,
            log_level: "DEBUG".to_string(),
            public_ip: bind_ip,
            bind_ip,
            port: bind_port,
            registry_url: mock_server.uri().parse().unwrap(),
            keypair_seed: hex::encode(rand::Rng::gen::<[u8; 10]>(&mut rand::thread_rng())),
            project_id: vars.project_id.into(),
            relay_url: vars.relay_url.parse().unwrap(),
            notify_url: notify_url.clone(),
            registry_auth_token: "".to_owned(),
            auth_redis_addr_read: None,
            auth_redis_addr_write: None,
            redis_pool_size: 1,
            telemetry_prometheus_port: None,
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

        Self {
            shutdown: signal,
            socket_addr,
            url: notify_url,
            postgres,
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
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &notify_server.postgres)
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
    )
    .await
    .unwrap();

    let accounts =
        get_subscriber_accounts_by_project_id(project_id.clone(), &notify_server.postgres)
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
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &notify_server.postgres)
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
    )
    .await
    .unwrap();

    let subscribers = get_subscriber_accounts_and_scopes_by_project_id(
        project_id.clone(),
        &notify_server.postgres,
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
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &notify_server.postgres)
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
    assert_eq!(claims.sub, format!("did:pkh:{account}"));
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
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &notify_server.postgres)
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
    assert_eq!(claims.msg.r#type, notification.r#type);
    assert_eq!(claims.msg.title, notification.title);
    assert_eq!(claims.msg.body, notification.body);
    assert_eq!(claims.msg.icon, "icon");
    assert_eq!(claims.msg.url, "url");
    assert!(claims.iat < chrono::Utc::now().timestamp() + JWT_LEEWAY); // TODO remove leeway
    assert!(claims.exp > chrono::Utc::now().timestamp() - JWT_LEEWAY); // TODO remove leeway
    assert_eq!(claims.app.as_ref(), app_domain);
    assert_eq!(claims.sub, format!("did:pkh:{account}"));
    assert_eq!(claims.act, "notify_message");
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
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &notify_server.postgres)
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
    let postgres = get_postgres().await;

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
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &postgres)
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
    )
    .await
    .unwrap();

    // Insert notify for delivery
    upsert_subscriber_notifications(notification_with_id.id, &[subscriber_id], &postgres)
        .await
        .unwrap();

    // Get the notify message for processing
    let processing_notify = pick_subscriber_notification_for_processing(&postgres)
        .await
        .unwrap();
    assert!(
        processing_notify.is_some(),
        "No notification for processing found"
    );

    // Run dead letter check and try to get another message for processing
    // and expect that there are no messages because the threshold is not reached
    let dead_letter_threshold_mins = 60; // one hour
    dead_letters_check(dead_letter_threshold_mins, &postgres)
        .await
        .unwrap();
    assert!(
        pick_subscriber_notification_for_processing(&postgres)
            .await
            .unwrap()
            .is_none(),
        "The messages should be already in the processing state and should not be picked"
    );

    // Manually change the `updated_at` date for the notify message to be older than the
    // dead letter threshold  plus 10 minutes
    let subscriber_notification_id = processing_notify.unwrap().subscriber_notification;
    let query = "UPDATE subscriber_notification SET updated_at = $1 WHERE id = $2";
    sqlx::query::<Postgres>(query)
        .bind(Utc::now() - Duration::minutes(dead_letter_threshold_mins as i64 + 10))
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
    dead_letters_check(dead_letter_threshold_mins, &postgres)
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
    let processing_notify = pick_subscriber_notification_for_processing(&postgres)
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
    // give up letter processing threshold plus 10 minutes
    let give_up_threshold_mins = 60 * 24; // one day

    let give_up_result_before = dead_letter_give_up_check(
        subscriber_notification_id,
        give_up_threshold_mins,
        &postgres,
    )
    .await
    .unwrap();
    assert!(!give_up_result_before);

    let query = "UPDATE subscriber_notification SET created_at = $1 WHERE id = $2";
    sqlx::query::<Postgres>(query)
        .bind(Utc::now() - Duration::minutes(give_up_threshold_mins as i64 + 10))
        .bind(subscriber_notification_id)
        .execute(&postgres)
        .await
        .unwrap();

    let give_up_result_after = dead_letter_give_up_check(
        subscriber_notification_id,
        give_up_threshold_mins,
        &postgres,
    )
    .await
    .unwrap();
    assert!(give_up_result_after);
}
