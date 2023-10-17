use {
    crate::{
        model::helpers::{get_project_by_project_id, upsert_project, upsert_subscriber},
        Result,
    },
    mongodb::bson::doc,
    serde::{Deserialize, Serialize},
    std::collections::HashSet,
};

#[derive(Serialize, Deserialize, Debug)]
pub struct ProjectData {
    #[serde(rename = "_id")]
    pub id: String,
    pub identity_keypair: Keypair,
    pub signing_keypair: Keypair,
    pub app_domain: String,
    pub topic: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Keypair {
    pub private_key: String,
    pub public_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClientData {
    #[serde(rename = "_id")]
    pub id: String,
    pub relay_url: String,
    pub sym_key: String,
    pub expiry: u64,
    pub scope: HashSet<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LookupEntry {
    #[serde(rename = "_id")]
    pub topic: String,
    pub project_id: String,
    pub account: String,
    pub expiry: u64,
}

pub async fn migrate(mongo: &mongodb::Database, postgres: &sqlx::PgPool) -> Result<()> {
    let mut projects_cursor = mongo
        .collection::<ProjectData>("project_data")
        .find(None, None)
        .await?;

    while projects_cursor.advance().await? {
        let project = projects_cursor.deserialize_current()?;

        upsert_project(
            project.id.clone().into(),
            &project.app_domain,
            project.topic.into(),
            project.identity_keypair.public_key,
            project.identity_keypair.private_key,
            project.signing_keypair.public_key,
            project.signing_keypair.private_key,
            postgres,
        )
        .await?;

        mongo
            .collection::<ProjectData>("project_data")
            .delete_one(doc! {"_id": project.id}, None)
            .await?;
    }

    let mut lookup_entry_cursor = mongo
        .collection::<LookupEntry>("lookup_table")
        .find(None, None)
        .await?;

    while lookup_entry_cursor.advance().await? {
        let lookup_entry = lookup_entry_cursor.deserialize_current()?;
        let client_data = mongo
            .collection::<ClientData>(&lookup_entry.project_id)
            .find_one(doc! {"_id": lookup_entry.account.clone()}, None)
            .await?
            .unwrap();

        let project =
            get_project_by_project_id(lookup_entry.project_id.clone().into(), postgres).await?;

        upsert_subscriber(
            project.id,
            client_data.id.into(),
            client_data.scope,
            &hex::decode(&client_data.sym_key)?.try_into().unwrap(),
            lookup_entry.topic.clone().into(),
            postgres,
        )
        .await?;

        mongo
            .collection::<LookupEntry>("lookup_table")
            .delete_one(doc! {"_id": lookup_entry.topic}, None)
            .await?;
        mongo
            .collection::<ClientData>(&lookup_entry.project_id)
            .delete_one(doc! {"_id": lookup_entry.account}, None)
            .await?;
    }

    Ok(())
}

#[cfg(all(test, feature = "storage"))]
mod test {
    use {
        super::ProjectData,
        crate::{
            migrate::{ClientData, Keypair, LookupEntry},
            model::{
                helpers::{
                    get_project_by_app_domain,
                    get_project_by_project_id,
                    get_project_by_topic,
                    get_project_topics,
                    get_subscriber_accounts_by_project_id,
                    get_subscriber_by_topic,
                    get_subscriber_topics,
                    get_subscribers_for_project_in,
                    get_subscriptions_by_account,
                },
                types::AccountId,
            },
        },
        chrono::{Duration, Utc},
        mongodb::{
            bson::doc,
            options::{ClientOptions, ResolverConfig},
        },
        relay_rpc::domain::{ProjectId, Topic},
        sqlx::{postgres::PgPoolOptions, PgPool},
        std::collections::HashSet,
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

    #[tokio::test]
    async fn test_empty_projects_and_subscribers() {
        let (mongodb, postgres) = get_dbs().await;
        super::migrate(&mongodb, &postgres).await.unwrap();
        assert_eq!(get_project_topics(&postgres).await.unwrap(), vec![]);
        assert_eq!(get_subscriber_topics(&postgres).await.unwrap(), vec![]);
    }

    #[tokio::test]
    async fn test_one_project() {
        let (mongodb, postgres) = get_dbs().await;

        let topic: Topic = "project_topic".into();
        let project_id: ProjectId = "project_id".into();
        let signing_secret = "signing_secret";
        let signing_public = "signing_public";
        let identity_secret = "identity_secret";
        let identity_public = "identity_public";
        let app_domain = "app.example.com";
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
                        private_key: identity_secret.to_string(),
                        public_key: identity_public.to_string(),
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

        super::migrate(&mongodb, &postgres).await.unwrap();

        assert_eq!(
            mongodb
                .collection::<ProjectData>("project_data")
                .count_documents(doc! {}, None)
                .await
                .unwrap(),
            0
        );

        assert_eq!(get_subscriber_topics(&postgres).await.unwrap(), vec![]);
        assert_eq!(get_project_topics(&postgres).await.unwrap(), vec![
            topic.clone()
        ]);
        let project = get_project_by_app_domain(app_domain, &postgres)
            .await
            .unwrap();
        assert_eq!(project.project_id, project_id.clone());
        assert_eq!(project.app_domain, app_domain);
        assert_eq!(project.topic, topic);
        assert_eq!(project.authentication_public_key, identity_public);
        assert_eq!(project.authentication_private_key, identity_secret);
        assert_eq!(project.subscribe_public_key, signing_public);
        assert_eq!(project.subscribe_private_key, signing_secret);

        let project = get_project_by_project_id(project_id.clone(), &postgres)
            .await
            .unwrap();
        assert_eq!(project.project_id, project_id.clone());
        assert_eq!(project.app_domain, app_domain);
        assert_eq!(project.topic, topic);
        assert_eq!(project.authentication_public_key, identity_public);
        assert_eq!(project.authentication_private_key, identity_secret);
        assert_eq!(project.subscribe_public_key, signing_public);
        assert_eq!(project.subscribe_private_key, signing_secret);

        let project = get_project_by_topic(topic.clone(), &postgres)
            .await
            .unwrap();
        assert_eq!(project.project_id, project_id.clone());
        assert_eq!(project.app_domain, app_domain);
        assert_eq!(project.topic, topic);
        assert_eq!(project.authentication_public_key, identity_public);
        assert_eq!(project.authentication_private_key, identity_secret);
        assert_eq!(project.subscribe_public_key, signing_public);
        assert_eq!(project.subscribe_private_key, signing_secret);
    }

    #[tokio::test]
    async fn test_one_subscriber() {
        let (mongodb, postgres) = get_dbs().await;

        let topic: Topic = "project_topic".into();
        let project_id: ProjectId = "project_id".into();
        let signing_secret = "signing_secret";
        let signing_public = "signing_public";
        let identity_secret = "identity_secret";
        let identity_public = "identity_public";
        let app_domain = "app.example.com";
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
                        private_key: identity_secret.to_string(),
                        public_key: identity_public.to_string(),
                    },
                    app_domain: app_domain.to_string(),
                    topic: topic.to_string(),
                },
                None,
            )
            .await
            .unwrap();

        let account_id: AccountId = "eip155:1:0xfff".into();
        let subscriber_sym_key = hex::encode([0u8; 32]);
        let subscriber_topic: Topic = "subscriber_topic".into();
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

        super::migrate(&mongodb, &postgres).await.unwrap();

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

        assert_eq!(get_subscriber_topics(&postgres).await.unwrap(), vec![
            subscriber_topic.clone()
        ]);

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

        let subscribers =
            get_subscribers_for_project_in(project.id, &[account_id.clone()], &postgres)
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

        let topic: Topic = "project_topic".into();
        let project_id: ProjectId = "project_id".into();
        let signing_secret = "signing_secret";
        let signing_public = "signing_public";
        let identity_secret = "identity_secret";
        let identity_public = "identity_public";
        let app_domain = "app.example.com";
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
                        private_key: identity_secret.to_string(),
                        public_key: identity_public.to_string(),
                    },
                    app_domain: app_domain.to_string(),
                    topic: topic.to_string(),
                },
                None,
            )
            .await
            .unwrap();

        let account_id: AccountId = "eip155:1:0xfff".into();
        let subscriber_sym_key = hex::encode([0u8; 32]);
        let subscriber_topic: Topic = "subscriber_topic".into();
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
        let subscriber_topic2: Topic = "subscriber_topic2".into();
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

        super::migrate(&mongodb, &postgres).await.unwrap();

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
}
