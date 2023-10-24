use {
    async_trait::async_trait,
    chrono::{Duration, Utc},
    hyper::StatusCode,
    mongodb::{
        bson::doc,
        options::{ClientOptions, ResolverConfig},
    },
    notify_server::{
        config::Configuration,
        migrate::{self, ClientData, Keypair, LookupEntry, ProjectData},
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
    },
    rand_chacha::rand_core::OsRng,
    relay_rpc::domain::{ProjectId, Topic},
    reqwest::Response,
    sqlx::{postgres::PgPoolOptions, PgPool},
    std::{
        collections::HashSet,
        net::{IpAddr, Ipv4Addr, SocketAddrV4},
    },
    test_context::{test_context, AsyncTestContext},
    tokio::{net::TcpListener, sync::broadcast, time::error::Elapsed},
    tracing_subscriber::fmt::format::FmtSpan,
    url::Url,
    uuid::Uuid,
};

async fn get_dbs() -> (mongodb::Database, PgPool) {
    let mongodb = mongodb::Client::with_options(
        ClientOptions::parse_with_resolver_config(
            &std::env::var("DATABASE_URL").unwrap(),
            ResolverConfig::cloudflare(),
        )
        .await
        .unwrap(),
    )
    .unwrap()
    .database("notify");
    mongodb.drop(None).await.unwrap();

    let postgres = PgPoolOptions::new()
        .connect(&std::env::var("POSTGRES_URL").unwrap())
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

    (mongodb, postgres)
}

fn generate_app_domain() -> String {
    format!(
        "{}.example.com",
        hex::encode(rand::Rng::gen::<[u8; 10]>(&mut rand::thread_rng()))
    )
}

fn generate_signing_keys() -> (String, String) {
    let signing_secret = x25519_dalek::StaticSecret::random_from_rng(OsRng);
    let signing_public = hex::encode(x25519_dalek::PublicKey::from(&signing_secret));
    let signing_secret = hex::encode(signing_secret);
    (signing_secret, signing_public)
}

fn generate_authentication_keys() -> (String, String) {
    let authentication_secret = ed25519_dalek::SigningKey::generate(&mut OsRng);
    let authentication_public = hex::encode(authentication_secret.verifying_key());
    let authentication_secret = hex::encode(authentication_secret.as_ref());
    (authentication_secret, authentication_public)
}

fn generate_account_id() -> AccountId {
    "eip155:1:0xfff".into()
}

#[tokio::test]
async fn test_empty_projects_and_subscribers() {
    let (mongodb, postgres) = get_dbs().await;
    notify_server::migrate::migrate(&mongodb, &postgres)
        .await
        .unwrap();
    assert_eq!(get_project_topics(&postgres).await.unwrap(), vec![]);
    assert_eq!(get_subscriber_topics(&postgres).await.unwrap(), vec![]);
}

#[tokio::test]
async fn test_one_project() {
    let (mongodb, postgres) = get_dbs().await;

    let topic = Topic::generate();
    let project_id = ProjectId::generate();
    let (signing_secret, signing_public) = generate_signing_keys();
    let (authentication_secret, authentication_public) = generate_authentication_keys();
    let app_domain = generate_app_domain();
    mongodb
        .collection::<ProjectData>("project_data")
        .insert_one(
            ProjectData {
                id: project_id.to_string(),
                signing_keypair: Keypair {
                    private_key: signing_secret.to_string(),
                    public_key: signing_public.to_string(),
                },
                identity_keypair: Keypair {
                    private_key: authentication_secret.to_string(),
                    public_key: authentication_public.to_string(),
                },
                app_domain: app_domain.to_string(),
                topic: topic.to_string(),
            },
            None,
        )
        .await
        .unwrap();

    assert_eq!(
        mongodb
            .collection::<ProjectData>("project_data")
            .count_documents(doc! {}, None)
            .await
            .unwrap(),
        1
    );

    migrate::migrate(&mongodb, &postgres).await.unwrap();

    assert_eq!(
        mongodb
            .collection::<ProjectData>("project_data")
            .count_documents(doc! {}, None)
            .await
            .unwrap(),
        0
    );

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
    assert_eq!(project.authentication_public_key, authentication_public);
    assert_eq!(project.authentication_private_key, authentication_secret);
    assert_eq!(project.subscribe_public_key, signing_public);
    assert_eq!(project.subscribe_private_key, signing_secret);

    let project = get_project_by_project_id(project_id.clone(), &postgres)
        .await
        .unwrap();
    assert_eq!(project.project_id, project_id.clone());
    assert_eq!(project.app_domain, app_domain);
    assert_eq!(project.topic, topic);
    assert_eq!(project.authentication_public_key, authentication_public);
    assert_eq!(project.authentication_private_key, authentication_secret);
    assert_eq!(project.subscribe_public_key, signing_public);
    assert_eq!(project.subscribe_private_key, signing_secret);

    let project = get_project_by_topic(topic.clone(), &postgres)
        .await
        .unwrap();
    assert_eq!(project.project_id, project_id.clone());
    assert_eq!(project.app_domain, app_domain);
    assert_eq!(project.topic, topic);
    assert_eq!(project.authentication_public_key, authentication_public);
    assert_eq!(project.authentication_private_key, authentication_secret);
    assert_eq!(project.subscribe_public_key, signing_public);
    assert_eq!(project.subscribe_private_key, signing_secret);
}

#[tokio::test]
async fn test_one_subscriber() {
    let (mongodb, postgres) = get_dbs().await;

    let topic = Topic::generate();
    let project_id = ProjectId::generate();
    let (signing_secret, signing_public) = generate_signing_keys();
    let (authentication_secret, authentication_public) = generate_authentication_keys();
    let app_domain = generate_app_domain();
    mongodb
        .collection::<ProjectData>("project_data")
        .insert_one(
            ProjectData {
                id: project_id.to_string(),
                signing_keypair: Keypair {
                    private_key: signing_secret.to_string(),
                    public_key: signing_public.to_string(),
                },
                identity_keypair: Keypair {
                    private_key: authentication_secret.to_string(),
                    public_key: authentication_public.to_string(),
                },
                app_domain: app_domain.to_string(),
                topic: topic.to_string(),
            },
            None,
        )
        .await
        .unwrap();

    let account_id = generate_account_id();
    let subscriber_sym_key = hex::encode([0u8; 32]);
    let subscriber_topic = Topic::generate();
    let subcriber_scope = HashSet::from(["scope1".to_string(), "scope2".to_string()]);
    let client_data = ClientData {
        id: account_id.to_string(),
        relay_url: "relay_url".to_string(),
        sym_key: subscriber_sym_key.to_string(),
        scope: subcriber_scope.clone(),
        expiry: 100,
    };
    mongodb
        .collection::<ClientData>(project_id.as_ref())
        .insert_one(client_data, None)
        .await
        .unwrap();
    assert_eq!(
        mongodb
            .collection::<ClientData>(project_id.as_ref())
            .count_documents(doc! {}, None)
            .await
            .unwrap(),
        1
    );
    mongodb
        .collection::<LookupEntry>("lookup_table")
        .insert_one(
            LookupEntry {
                topic: subscriber_topic.to_string(),
                project_id: project_id.to_string(),
                account: account_id.to_string(),
                expiry: 100,
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        mongodb
            .collection::<LookupEntry>("lookup_table")
            .count_documents(doc! {}, None)
            .await
            .unwrap(),
        1
    );

    migrate::migrate(&mongodb, &postgres).await.unwrap();

    assert_eq!(
        mongodb
            .collection::<ClientData>(project_id.as_ref())
            .count_documents(doc! {}, None)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        mongodb
            .collection::<LookupEntry>("lookup_table")
            .count_documents(doc! {}, None)
            .await
            .unwrap(),
        0
    );

    let project = get_project_by_project_id(project_id.clone(), &postgres)
        .await
        .unwrap();

    assert_eq!(
        get_subscriber_topics(&postgres).await.unwrap(),
        vec![subscriber_topic.clone()]
    );

    let subscriber = get_subscriber_by_topic(subscriber_topic.clone(), &postgres)
        .await
        .unwrap();
    assert_eq!(subscriber.project, project.id);
    assert_eq!(subscriber.account, account_id);
    assert_eq!(subscriber.sym_key, subscriber_sym_key);
    assert_eq!(subscriber.topic, subscriber_topic);
    assert_eq!(
        subscriber.scope.into_iter().collect::<HashSet<_>>(),
        subcriber_scope
    );
    assert!(subscriber.expiry > Utc::now() + Duration::days(29));

    let subscribers = get_subscribers_for_project_in(project.id, &[account_id.clone()], &postgres)
        .await
        .unwrap();
    assert_eq!(subscribers.len(), 1);
    let subscriber = &subscribers[0];
    assert_eq!(subscriber.project, project.id);
    assert_eq!(subscriber.account, account_id);
    assert_eq!(subscriber.sym_key, subscriber_sym_key);
    assert_eq!(subscriber.topic, subscriber_topic);
    assert_eq!(
        subscriber.scope.iter().cloned().collect::<HashSet<_>>(),
        subcriber_scope
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
    assert_eq!(subscriber.sym_key, subscriber_sym_key);
    assert_eq!(
        subscriber.scope.iter().cloned().collect::<HashSet<_>>(),
        subcriber_scope
    );
    assert!(subscriber.expiry > Utc::now() + Duration::days(29));
}

#[tokio::test]
async fn test_two_subscribers() {
    let (mongodb, postgres) = get_dbs().await;

    let topic = Topic::generate();
    let project_id = ProjectId::generate();
    let (signing_secret, signing_public) = generate_signing_keys();
    let (authentication_secret, authentication_public) = generate_authentication_keys();
    let app_domain = generate_app_domain();
    mongodb
        .collection::<ProjectData>("project_data")
        .insert_one(
            ProjectData {
                id: project_id.to_string(),
                signing_keypair: Keypair {
                    private_key: signing_secret.to_string(),
                    public_key: signing_public.to_string(),
                },
                identity_keypair: Keypair {
                    private_key: authentication_secret.to_string(),
                    public_key: authentication_public.to_string(),
                },
                app_domain: app_domain.to_string(),
                topic: topic.to_string(),
            },
            None,
        )
        .await
        .unwrap();

    let account_id = generate_account_id();
    let subscriber_sym_key = hex::encode([0u8; 32]);
    let subscriber_topic = Topic::generate();
    let subcriber_scope = HashSet::from(["scope1".to_string(), "scope2".to_string()]);
    let client_data = ClientData {
        id: account_id.to_string(),
        relay_url: "relay_url".to_string(),
        sym_key: subscriber_sym_key.to_string(),
        scope: subcriber_scope.clone(),
        expiry: 100,
    };
    mongodb
        .collection::<ClientData>(project_id.as_ref())
        .insert_one(client_data, None)
        .await
        .unwrap();
    mongodb
        .collection::<LookupEntry>("lookup_table")
        .insert_one(
            LookupEntry {
                topic: subscriber_topic.to_string(),
                project_id: project_id.to_string(),
                account: account_id.to_string(),
                expiry: 100,
            },
            None,
        )
        .await
        .unwrap();

    let account_id2: AccountId = "eip155:1:0xEEE".into();
    let subscriber_sym_key2 = hex::encode([1u8; 32]);
    let subscriber_topic2 = Topic::generate();
    let subcriber_scope2 = HashSet::from(["scope12".to_string(), "scope22".to_string()]);
    let client_data2 = ClientData {
        id: account_id2.to_string(),
        relay_url: "relay_url".to_string(),
        sym_key: subscriber_sym_key2.to_string(),
        scope: subcriber_scope2.clone(),
        expiry: 100,
    };
    mongodb
        .collection::<ClientData>(project_id.as_ref())
        .insert_one(client_data2, None)
        .await
        .unwrap();
    mongodb
        .collection::<LookupEntry>("lookup_table")
        .insert_one(
            LookupEntry {
                topic: subscriber_topic2.to_string(),
                project_id: project_id.to_string(),
                account: account_id2.to_string(),
                expiry: 100,
            },
            None,
        )
        .await
        .unwrap();

    assert_eq!(
        mongodb
            .collection::<ClientData>(project_id.as_ref())
            .count_documents(doc! {}, None)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        mongodb
            .collection::<LookupEntry>("lookup_table")
            .count_documents(doc! {}, None)
            .await
            .unwrap(),
        2
    );

    migrate::migrate(&mongodb, &postgres).await.unwrap();

    assert_eq!(
        mongodb
            .collection::<ClientData>(project_id.as_ref())
            .count_documents(doc! {}, None)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        mongodb
            .collection::<LookupEntry>("lookup_table")
            .count_documents(doc! {}, None)
            .await
            .unwrap(),
        0
    );

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
    assert_eq!(subscriber.sym_key, subscriber_sym_key);
    assert_eq!(subscriber.topic, subscriber_topic);
    assert_eq!(
        subscriber.scope.into_iter().collect::<HashSet<_>>(),
        subcriber_scope
    );
    assert!(subscriber.expiry > Utc::now() + Duration::days(29));

    let subscriber = get_subscriber_by_topic(subscriber_topic2.clone(), &postgres)
        .await
        .unwrap();
    assert_eq!(subscriber.project, project.id);
    assert_eq!(subscriber.account, account_id2);
    assert_eq!(subscriber.sym_key, subscriber_sym_key2);
    assert_eq!(subscriber.topic, subscriber_topic2);
    assert_eq!(
        subscriber.scope.into_iter().collect::<HashSet<_>>(),
        subcriber_scope2
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
            assert_eq!(subscriber.sym_key, subscriber_sym_key);
            assert_eq!(subscriber.topic, subscriber_topic);
            assert_eq!(
                subscriber.scope.iter().cloned().collect::<HashSet<_>>(),
                subcriber_scope
            );
            assert!(subscriber.expiry > Utc::now() + Duration::days(29));
        } else {
            assert_eq!(subscriber.project, project.id);
            assert_eq!(subscriber.sym_key, subscriber_sym_key2);
            assert_eq!(subscriber.topic, subscriber_topic2);
            assert_eq!(
                subscriber.scope.iter().cloned().collect::<HashSet<_>>(),
                subcriber_scope2
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
    assert_eq!(subscriber.sym_key, subscriber_sym_key);
    assert_eq!(
        subscriber.scope.iter().cloned().collect::<HashSet<_>>(),
        subcriber_scope
    );
    assert!(subscriber.expiry > Utc::now() + Duration::days(29));

    let subscribers = get_subscriptions_by_account(account_id2.clone(), &postgres)
        .await
        .unwrap();
    assert_eq!(subscribers.len(), 1);
    let subscriber = &subscribers[0];
    assert_eq!(subscriber.app_domain, project.app_domain);
    assert_eq!(subscriber.account, account_id2);
    assert_eq!(subscriber.sym_key, subscriber_sym_key2);
    assert_eq!(
        subscriber.scope.iter().cloned().collect::<HashSet<_>>(),
        subcriber_scope2
    );
    assert!(subscriber.expiry > Utc::now() + Duration::days(29));
}

#[tokio::test]
async fn test_one_subscriber_two_projects() {
    let (mongodb, postgres) = get_dbs().await;

    let topic = Topic::generate();
    let project_id = ProjectId::generate();
    let (signing_secret, signing_public) = generate_signing_keys();
    let (authentication_secret, authentication_public) = generate_authentication_keys();
    let app_domain = generate_app_domain();
    mongodb
        .collection::<ProjectData>("project_data")
        .insert_one(
            ProjectData {
                id: project_id.to_string(),
                signing_keypair: Keypair {
                    private_key: signing_secret.to_string(),
                    public_key: signing_public.to_string(),
                },
                identity_keypair: Keypair {
                    private_key: authentication_secret.to_string(),
                    public_key: authentication_public.to_string(),
                },
                app_domain: app_domain.to_string(),
                topic: topic.to_string(),
            },
            None,
        )
        .await
        .unwrap();

    let topic2 = Topic::generate();
    let project_id2 = ProjectId::generate();
    let (signing_secret2, signing_public2) = generate_signing_keys();
    let (authentication_secret2, authentication_public2) = generate_authentication_keys();
    let app_domain2 = generate_app_domain();
    mongodb
        .collection::<ProjectData>("project_data")
        .insert_one(
            ProjectData {
                id: project_id2.to_string(),
                signing_keypair: Keypair {
                    private_key: signing_secret2.to_string(),
                    public_key: signing_public2.to_string(),
                },
                identity_keypair: Keypair {
                    private_key: authentication_secret2.to_string(),
                    public_key: authentication_public2.to_string(),
                },
                app_domain: app_domain2.to_string(),
                topic: topic2.to_string(),
            },
            None,
        )
        .await
        .unwrap();

    let account_id = generate_account_id();
    let subscriber_sym_key = hex::encode([0u8; 32]);
    let subscriber_topic = Topic::generate();
    let subcriber_scope = HashSet::from(["scope1".to_string(), "scope2".to_string()]);
    let client_data = ClientData {
        id: account_id.to_string(),
        relay_url: "relay_url".to_string(),
        sym_key: subscriber_sym_key.to_string(),
        scope: subcriber_scope.clone(),
        expiry: 100,
    };
    mongodb
        .collection::<ClientData>(project_id.as_ref())
        .insert_one(client_data, None)
        .await
        .unwrap();
    mongodb
        .collection::<LookupEntry>("lookup_table")
        .insert_one(
            LookupEntry {
                topic: subscriber_topic.to_string(),
                project_id: project_id.to_string(),
                account: account_id.to_string(),
                expiry: 100,
            },
            None,
        )
        .await
        .unwrap();
    let subscriber_sym_key2 = hex::encode([1u8; 32]);
    let subscriber_topic2 = Topic::generate();
    let subcriber_scope2 = HashSet::from(["scope12".to_string(), "scope22".to_string()]);
    let client_data2 = ClientData {
        id: account_id.to_string(),
        relay_url: "relay_url".to_string(),
        sym_key: subscriber_sym_key2.to_string(),
        scope: subcriber_scope2.clone(),
        expiry: 100,
    };
    mongodb
        .collection::<ClientData>(project_id2.as_ref())
        .insert_one(client_data2, None)
        .await
        .unwrap();
    mongodb
        .collection::<LookupEntry>("lookup_table")
        .insert_one(
            LookupEntry {
                topic: subscriber_topic2.to_string(),
                project_id: project_id2.to_string(),
                account: account_id.to_string(),
                expiry: 100,
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        mongodb
            .collection::<ClientData>(project_id.as_ref())
            .count_documents(doc! {}, None)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        mongodb
            .collection::<ClientData>(project_id2.as_ref())
            .count_documents(doc! {}, None)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        mongodb
            .collection::<LookupEntry>("lookup_table")
            .count_documents(doc! {}, None)
            .await
            .unwrap(),
        2
    );

    migrate::migrate(&mongodb, &postgres).await.unwrap();

    assert_eq!(
        mongodb
            .collection::<ClientData>(project_id.as_ref())
            .count_documents(doc! {}, None)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        mongodb
            .collection::<LookupEntry>("lookup_table")
            .count_documents(doc! {}, None)
            .await
            .unwrap(),
        0
    );

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
    assert_eq!(subscriber.sym_key, subscriber_sym_key);
    assert_eq!(subscriber.topic, subscriber_topic);
    assert_eq!(
        subscriber.scope.into_iter().collect::<HashSet<_>>(),
        subcriber_scope
    );
    assert!(subscriber.expiry > Utc::now() + Duration::days(29));

    let subscriber = get_subscriber_by_topic(subscriber_topic2.clone(), &postgres)
        .await
        .unwrap();
    assert_eq!(subscriber.project, project2.id);
    assert_eq!(subscriber.account, account_id);
    assert_eq!(subscriber.sym_key, subscriber_sym_key2);
    assert_eq!(subscriber.topic, subscriber_topic2);
    assert_eq!(
        subscriber.scope.into_iter().collect::<HashSet<_>>(),
        subcriber_scope2
    );
    assert!(subscriber.expiry > Utc::now() + Duration::days(29));

    let subscribers = get_subscribers_for_project_in(project.id, &[account_id.clone()], &postgres)
        .await
        .unwrap();
    assert_eq!(subscribers.len(), 1);
    let subscriber = &subscribers[0];
    assert_eq!(subscriber.project, project.id);
    assert_eq!(subscriber.account, account_id);
    assert_eq!(subscriber.sym_key, subscriber_sym_key);
    assert_eq!(subscriber.topic, subscriber_topic);
    assert_eq!(
        subscriber.scope.iter().cloned().collect::<HashSet<_>>(),
        subcriber_scope
    );
    assert!(subscriber.expiry > Utc::now() + Duration::days(29));

    let subscribers = get_subscribers_for_project_in(project2.id, &[account_id.clone()], &postgres)
        .await
        .unwrap();
    assert_eq!(subscribers.len(), 1);
    let subscriber = &subscribers[0];
    assert_eq!(subscriber.project, project2.id);
    assert_eq!(subscriber.account, account_id);
    assert_eq!(subscriber.sym_key, subscriber_sym_key2);
    assert_eq!(subscriber.topic, subscriber_topic2);
    assert_eq!(
        subscriber.scope.iter().cloned().collect::<HashSet<_>>(),
        subcriber_scope2
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
            assert_eq!(subscriber.sym_key, subscriber_sym_key);
            assert_eq!(
                subscriber.scope.iter().cloned().collect::<HashSet<_>>(),
                subcriber_scope
            );
            assert!(subscriber.expiry > Utc::now() + Duration::days(29));
        } else {
            assert_eq!(subscriber.app_domain, app_domain2);
            assert_eq!(subscriber.account, account_id);
            assert_eq!(subscriber.sym_key, subscriber_sym_key2);
            assert_eq!(
                subscriber.scope.iter().cloned().collect::<HashSet<_>>(),
                subcriber_scope2
            );
            assert!(subscriber.expiry > Utc::now() + Duration::days(29));
        }
    }
}

#[tokio::test]
async fn test_call_migrate_twice() {
    let (mongodb, postgres) = get_dbs().await;

    let topic = Topic::generate();
    let project_id = ProjectId::generate();
    let (signing_secret, signing_public) = generate_signing_keys();
    let (authentication_secret, authentication_public) = generate_authentication_keys();
    let app_domain = generate_app_domain();
    mongodb
        .collection::<ProjectData>("project_data")
        .insert_one(
            ProjectData {
                id: project_id.to_string(),
                signing_keypair: Keypair {
                    private_key: signing_secret.to_string(),
                    public_key: signing_public.to_string(),
                },
                identity_keypair: Keypair {
                    private_key: authentication_secret.to_string(),
                    public_key: authentication_public.to_string(),
                },
                app_domain: app_domain.to_string(),
                topic: topic.to_string(),
            },
            None,
        )
        .await
        .unwrap();

    let topic2 = Topic::generate();
    let project_id2 = ProjectId::generate();
    let (signing_secret2, signing_public2) = generate_signing_keys();
    let (authentication_secret2, authentication_public2) = generate_authentication_keys();
    let app_domain2 = generate_app_domain();
    mongodb
        .collection::<ProjectData>("project_data")
        .insert_one(
            ProjectData {
                id: project_id2.to_string(),
                signing_keypair: Keypair {
                    private_key: signing_secret2.to_string(),
                    public_key: signing_public2.to_string(),
                },
                identity_keypair: Keypair {
                    private_key: authentication_secret2.to_string(),
                    public_key: authentication_public2.to_string(),
                },
                app_domain: app_domain2.to_string(),
                topic: topic2.to_string(),
            },
            None,
        )
        .await
        .unwrap();

    let account_id = generate_account_id();
    let subscriber_sym_key = hex::encode([0u8; 32]);
    let subscriber_topic = Topic::generate();
    let subcriber_scope = HashSet::from(["scope1".to_string(), "scope2".to_string()]);
    let client_data = ClientData {
        id: account_id.to_string(),
        relay_url: "relay_url".to_string(),
        sym_key: subscriber_sym_key.to_string(),
        scope: subcriber_scope.clone(),
        expiry: 100,
    };
    mongodb
        .collection::<ClientData>(project_id.as_ref())
        .insert_one(client_data, None)
        .await
        .unwrap();
    mongodb
        .collection::<LookupEntry>("lookup_table")
        .insert_one(
            LookupEntry {
                topic: subscriber_topic.to_string(),
                project_id: project_id.to_string(),
                account: account_id.to_string(),
                expiry: 100,
            },
            None,
        )
        .await
        .unwrap();
    let subscriber_sym_key2 = hex::encode([1u8; 32]);
    let subscriber_topic2 = Topic::generate();
    let subcriber_scope2 = HashSet::from(["scope12".to_string(), "scope22".to_string()]);
    let client_data2 = ClientData {
        id: account_id.to_string(),
        relay_url: "relay_url".to_string(),
        sym_key: subscriber_sym_key2.to_string(),
        scope: subcriber_scope2.clone(),
        expiry: 100,
    };
    mongodb
        .collection::<ClientData>(project_id2.as_ref())
        .insert_one(client_data2, None)
        .await
        .unwrap();
    mongodb
        .collection::<LookupEntry>("lookup_table")
        .insert_one(
            LookupEntry {
                topic: subscriber_topic2.to_string(),
                project_id: project_id2.to_string(),
                account: account_id.to_string(),
                expiry: 100,
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        mongodb
            .collection::<ClientData>(project_id.as_ref())
            .count_documents(doc! {}, None)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        mongodb
            .collection::<ClientData>(project_id2.as_ref())
            .count_documents(doc! {}, None)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        mongodb
            .collection::<LookupEntry>("lookup_table")
            .count_documents(doc! {}, None)
            .await
            .unwrap(),
        2
    );

    migrate::migrate(&mongodb, &postgres).await.unwrap();

    // Call it again to make sure this is idempotent
    migrate::migrate(&mongodb, &postgres).await.unwrap();

    assert_eq!(
        mongodb
            .collection::<ClientData>(project_id.as_ref())
            .count_documents(doc! {}, None)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        mongodb
            .collection::<LookupEntry>("lookup_table")
            .count_documents(doc! {}, None)
            .await
            .unwrap(),
        0
    );

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
    assert_eq!(subscriber.sym_key, subscriber_sym_key);
    assert_eq!(subscriber.topic, subscriber_topic);
    assert_eq!(
        subscriber.scope.into_iter().collect::<HashSet<_>>(),
        subcriber_scope
    );
    assert!(subscriber.expiry > Utc::now() + Duration::days(29));

    let subscriber = get_subscriber_by_topic(subscriber_topic2.clone(), &postgres)
        .await
        .unwrap();
    assert_eq!(subscriber.project, project2.id);
    assert_eq!(subscriber.account, account_id);
    assert_eq!(subscriber.sym_key, subscriber_sym_key2);
    assert_eq!(subscriber.topic, subscriber_topic2);
    assert_eq!(
        subscriber.scope.into_iter().collect::<HashSet<_>>(),
        subcriber_scope2
    );
    assert!(subscriber.expiry > Utc::now() + Duration::days(29));

    let subscribers = get_subscribers_for_project_in(project.id, &[account_id.clone()], &postgres)
        .await
        .unwrap();
    assert_eq!(subscribers.len(), 1);
    let subscriber = &subscribers[0];
    assert_eq!(subscriber.project, project.id);
    assert_eq!(subscriber.account, account_id);
    assert_eq!(subscriber.sym_key, subscriber_sym_key);
    assert_eq!(subscriber.topic, subscriber_topic);
    assert_eq!(
        subscriber.scope.iter().cloned().collect::<HashSet<_>>(),
        subcriber_scope
    );
    assert!(subscriber.expiry > Utc::now() + Duration::days(29));

    let subscribers = get_subscribers_for_project_in(project2.id, &[account_id.clone()], &postgres)
        .await
        .unwrap();
    assert_eq!(subscribers.len(), 1);
    let subscriber = &subscribers[0];
    assert_eq!(subscriber.project, project2.id);
    assert_eq!(subscriber.account, account_id);
    assert_eq!(subscriber.sym_key, subscriber_sym_key2);
    assert_eq!(subscriber.topic, subscriber_topic2);
    assert_eq!(
        subscriber.scope.iter().cloned().collect::<HashSet<_>>(),
        subcriber_scope2
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
            assert_eq!(subscriber.sym_key, subscriber_sym_key);
            assert_eq!(
                subscriber.scope.iter().cloned().collect::<HashSet<_>>(),
                subcriber_scope
            );
            assert!(subscriber.expiry > Utc::now() + Duration::days(29));
        } else {
            assert_eq!(subscriber.app_domain, app_domain2);
            assert_eq!(subscriber.account, account_id);
            assert_eq!(subscriber.sym_key, subscriber_sym_key2);
            assert_eq!(
                subscriber.scope.iter().cloned().collect::<HashSet<_>>(),
                subcriber_scope2
            );
            assert!(subscriber.expiry > Utc::now() + Duration::days(29));
        }
    }
}

async fn is_port_available(port: u16) -> bool {
    TcpListener::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port))
        .await
        .is_ok()
}

async fn find_free_port() -> u16 {
    use std::sync::atomic::{AtomicU16, Ordering};
    static NEXT_PORT: AtomicU16 = AtomicU16::new(9000);
    loop {
        let port = NEXT_PORT.fetch_add(1, Ordering::SeqCst);
        if is_port_available(port).await {
            return port;
        }
    }
}

async fn wait_for_port_to_be(port: u16, open: bool) -> Result<(), Elapsed> {
    use {std::time::Duration, tokio::time};
    time::timeout(Duration::from_secs(3), async {
        while is_port_available(port).await != open {
            time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
}

struct NotifyServerContext {
    shutdown: broadcast::Sender<()>,
    port: u16,
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

        let public_ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let config = Configuration {
            postgres_url: std::env::var("POSTGRES_URL").unwrap(),
            log_level: "trace".to_string(),
            public_ip,
            port: find_free_port().await,
            registry_url: mock_server.uri(),
            database_url: std::env::var("DATABASE_URL").unwrap(),
            keypair_seed: hex::encode(rand::Rng::gen::<[u8; 10]>(&mut rand::thread_rng())),
            project_id: std::env::var("PROJECT_ID").unwrap(),
            relay_url: std::env::var("RELAY_URL").unwrap(),
            notify_url: format!("http://{public_ip}"),
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

        wait_for_port_to_be(config.port, false).await.unwrap();

        let postgres = PgPoolOptions::new()
            .connect(&config.postgres_url)
            .await
            .unwrap();

        Self {
            shutdown: signal,
            port: config.port,
            url: Url::parse(&format!("http://{}:{}", config.public_ip, config.port)).unwrap(),
            postgres,
        }
    }

    async fn teardown(mut self) {
        self.shutdown.send(()).unwrap();
        wait_for_port_to_be(self.port, true).await.unwrap();
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
    let (signing_secret, signing_public) = generate_signing_keys();
    let (authentication_secret, authentication_public) = generate_authentication_keys();
    upsert_project(
        project_id.clone(),
        app_domain,
        topic,
        authentication_public,
        authentication_secret,
        signing_public,
        signing_secret,
        &notify_server.postgres,
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &notify_server.postgres)
        .await
        .unwrap();

    let account = generate_account_id();
    let scope = HashSet::from(["scope1".to_string(), "scope2".to_string()]);
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
    let (signing_secret, signing_public) = generate_signing_keys();
    let (authentication_secret, authentication_public) = generate_authentication_keys();
    upsert_project(
        project_id.clone(),
        app_domain,
        topic,
        authentication_public,
        authentication_secret,
        signing_public,
        signing_secret,
        &notify_server.postgres,
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &notify_server.postgres)
        .await
        .unwrap();

    let account = generate_account_id();
    let scope = HashSet::from(["scope1".to_string(), "scope2".to_string()]);
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

#[tokio::test]
async fn test_get_subscriber_accounts_by_project_id() {
    let (_, postgres) = get_dbs().await;

    let project_id = ProjectId::generate();
    let app_domain = &generate_app_domain();
    let topic = Topic::generate();
    let (signing_secret, signing_public) = generate_signing_keys();
    let (authentication_secret, authentication_public) = generate_authentication_keys();
    upsert_project(
        project_id.clone(),
        app_domain,
        topic,
        authentication_public,
        authentication_secret,
        signing_public,
        signing_secret,
        &postgres,
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &postgres)
        .await
        .unwrap();

    let account = generate_account_id();
    let scope = HashSet::from(["scope1".to_string(), "scope2".to_string()]);
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
    let (_, postgres) = get_dbs().await;

    let project_id = ProjectId::generate();
    let app_domain = &generate_app_domain();
    let topic = Topic::generate();
    let (signing_secret, signing_public) = generate_signing_keys();
    let (authentication_secret, authentication_public) = generate_authentication_keys();
    upsert_project(
        project_id.clone(),
        app_domain,
        topic,
        authentication_public,
        authentication_secret,
        signing_public,
        signing_secret,
        &postgres,
    )
    .await
    .unwrap();
    let project = get_project_by_project_id(project_id.clone(), &postgres)
        .await
        .unwrap();

    let account = generate_account_id();
    let scope = HashSet::from(["scope1".to_string(), "scope2".to_string()]);
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
