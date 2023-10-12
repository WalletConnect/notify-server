use {
    super::types::{Project, Subscriber},
    crate::model::types::AccountId,
    chrono::{DateTime, Utc},
    relay_rpc::domain::{ProjectId, Topic},
    sqlx::{FromRow, PgPool, Postgres},
    uuid::Uuid,
};

#[derive(Debug, FromRow)]
pub struct ProjectWithPublicKeys {
    pub authentication_public_key: String,
    pub subscribe_public_key: String,
}

#[allow(clippy::too_many_arguments)]
pub async fn upsert_project(
    project_id: ProjectId,
    app_domain: &str,
    topic: Topic,
    identity_public: String,
    identity_secret: String,
    signing_public: String,
    signing_secret: String,
    postgres: &PgPool,
) -> Result<ProjectWithPublicKeys, sqlx::error::Error> {
    let query = "
        INSERT INTO project (
            project_id,
            app_domain,
            topic,
            authentication_public_key,
            authentication_private_key,
            subscribe_public_key,
            subscribe_private_key
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        ON CONFLICT (project_id) DO UPDATE SET
            app_domain = $2,
            updated_at = now()
        RETURNING authentication_public_key, subscribe_public_key
    ";
    sqlx::query_as::<Postgres, ProjectWithPublicKeys>(query)
        .bind(project_id.as_ref())
        .bind(app_domain)
        .bind(topic.as_ref())
        .bind(identity_public)
        .bind(identity_secret)
        .bind(signing_public)
        .bind(signing_secret)
        .fetch_one(postgres)
        .await
}

pub async fn get_project_by_id(id: Uuid, postgres: &PgPool) -> Result<Project, sqlx::error::Error> {
    let query = "
        SELECT *
        FROM project
        WHERE id=$1
    ";
    sqlx::query_as::<Postgres, Project>(query)
        .bind(id)
        .fetch_one(postgres)
        .await
}

pub async fn get_project_by_project_id(
    project_id: ProjectId,
    postgres: &PgPool,
) -> Result<Project, sqlx::error::Error> {
    let query = "
        SELECT *
        FROM project
        WHERE project_id=$1
    ";
    sqlx::query_as::<Postgres, Project>(query)
        .bind(project_id.as_ref())
        .fetch_one(postgres)
        .await
}

pub async fn get_project_by_app_domain(
    app_domain: &str,
    postgres: &PgPool,
) -> Result<Project, sqlx::error::Error> {
    let query = "
        SELECT *
        FROM project
        WHERE app_domain=$1
    ";
    sqlx::query_as::<Postgres, Project>(query)
        .bind(app_domain)
        .fetch_one(postgres)
        .await
}

pub async fn get_project_by_topic(
    topic: Topic,
    postgres: &PgPool,
) -> Result<Project, sqlx::error::Error> {
    let query = "
        SELECT *
        FROM project
        WHERE topic=$1
    ";
    sqlx::query_as::<Postgres, Project>(query)
        .bind(topic.as_ref())
        .fetch_one(postgres)
        .await
}

pub async fn get_subscriber_accounts_by_project_id(
    project_id: ProjectId,
    postgres: &PgPool,
) -> Result<Vec<AccountId>, sqlx::error::Error> {
    #[derive(Debug, FromRow)]
    struct ProjectWithAccount {
        #[sqlx(try_from = "String")]
        account: AccountId,
    }
    let query = "
        SELECT account
        FROM subscriber
        JOIN project ON project.id=subscriber.project
        WHERE project.project_id=$1
    ";
    let projects = sqlx::query_as::<Postgres, ProjectWithAccount>(query)
        .bind(project_id.as_ref())
        .fetch_all(postgres)
        .await?;
    Ok(projects.into_iter().map(|p| p.account).collect())
}

pub async fn get_subscriber_topics(postgres: &PgPool) -> Result<Vec<Topic>, sqlx::error::Error> {
    #[derive(Debug, FromRow)]
    struct SubscriberWithTopic {
        #[sqlx(try_from = "String")]
        topic: Topic,
    }
    let query = "
        SELECT topic
        FROM subscriber
    ";
    let subscribers = sqlx::query_as::<Postgres, SubscriberWithTopic>(query)
        .fetch_all(postgres)
        .await?;
    Ok(subscribers.into_iter().map(|p| p.topic).collect())
}

pub async fn get_project_topics(postgres: &PgPool) -> Result<Vec<Topic>, sqlx::error::Error> {
    #[derive(Debug, FromRow)]
    struct ProjectWithTopic {
        #[sqlx(try_from = "String")]
        topic: Topic,
    }
    let query = "
        SELECT topic
        FROM project
    ";
    let projects = sqlx::query_as::<Postgres, ProjectWithTopic>(query)
        .fetch_all(postgres)
        .await?;
    Ok(projects.into_iter().map(|p| p.topic).collect())
}

// FIXME idempotency
pub async fn upsert_subscriber(
    project: Uuid,
    account: AccountId,
    notify_key: &[u8; 32],
    notify_topic: Topic,
    postgres: &PgPool,
) -> Result<Uuid, sqlx::error::Error> {
    #[derive(Debug, FromRow)]
    struct SubscriberWithId {
        id: Uuid,
    }
    let subscriber = sqlx::query_as::<Postgres, SubscriberWithId>(
        "
            INSERT INTO subscriber (
                project,
                account,
                sym_key,
                topic,
                expiry
            )
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id
        ",
    )
    .bind(project)
    .bind(account.as_ref())
    .bind(hex::encode(notify_key))
    .bind(notify_topic.as_ref())
    .bind(Utc::now() + chrono::Duration::days(30))
    // FIXME scopes
    .fetch_one(postgres)
    .await?;
    Ok(subscriber.id)
}

// FIXME idempotency
pub async fn update_subscriber(
    project: Uuid,
    account: AccountId,
    postgres: &PgPool,
) -> Result<(), sqlx::error::Error> {
    let _ = sqlx::query::<Postgres>(
        "
            UPDATE subscriber
            SET expiry=$1
            WHERE project=$2 AND account=$3
        ",
    )
    // FIXME scopes
    .bind(Utc::now() + chrono::Duration::days(30))
    .bind(project)
    .bind(account.as_ref())
    .execute(postgres)
    .await?;
    Ok(())
}

pub async fn delete_subscriber(
    subscriber: Uuid,
    postgres: &PgPool,
) -> Result<(), sqlx::error::Error> {
    let query = "
        DELETE FROM subscriber
        WHERE id=$1
    ";
    let _ = sqlx::query::<Postgres>(query)
        .bind(subscriber)
        .execute(postgres)
        .await?;
    Ok(())
}

pub async fn get_subscriber_by_topic(
    topic: Topic,
    postgres: &PgPool,
) -> Result<Subscriber, sqlx::error::Error> {
    let query = "
        SELECT *
        FROM subscriber
        WHERE topic=$1
    ";
    sqlx::query_as::<Postgres, Subscriber>(query)
        .bind(topic.as_ref())
        .fetch_one(postgres)
        .await
}

pub async fn get_subscribers_for_project_in(
    project: Uuid,
    accounts: &[AccountId],
    postgres: &PgPool,
) -> Result<Vec<Subscriber>, sqlx::error::Error> {
    let query = "
        SELECT *
        FROM subscriber
        WHERE project=$1 AND account = ANY($2)
    ";
    sqlx::query_as::<Postgres, Subscriber>(query)
        .bind(project)
        .bind(accounts.iter().map(|a| a.as_ref()).collect::<Vec<_>>())
        .fetch_all(postgres)
        .await
}

#[derive(FromRow)]
pub struct SubscriberWithProject {
    /// dApp url that the subscription refers to
    pub app_domain: String,
    /// Symetric key used for notify topic. sha256 to get notify topic to manage
    /// the subscription and call wc_notifySubscriptionUpdate and
    /// wc_notifySubscriptionDelete
    pub sym_key: String,
    /// CAIP-10 account
    pub account: String, // TODO do we need to return this?
    /// Array of notification types enabled for this subscription
    // pub scope: Vec<String>, // FIXME
    /// Unix timestamp of expiration
    pub expiry: DateTime<Utc>,
}

pub async fn get_subscriptions_by_account(
    account: AccountId,
    postgres: &PgPool,
) -> Result<Vec<SubscriberWithProject>, sqlx::error::Error> {
    // FIXME scope
    let query: &str = "
        SELECT app_domain, sym_key, account, expiry
        FROM subscriber
        JOIN project ON project.id=subscriber.project
        WHERE account=$1
    ";
    sqlx::query_as::<Postgres, SubscriberWithProject>(query)
        .bind(account.as_ref())
        .fetch_all(postgres)
        .await
}

pub async fn get_subscriptions_by_account_and_app(
    account: AccountId,
    app_domain: &str,
    postgres: &PgPool,
) -> Result<Vec<SubscriberWithProject>, sqlx::error::Error> {
    // FIXME scope
    let query: &str = "
        SELECT app_domain, sym_key, account, expiry
        FROM subscriber
        JOIN project ON project.id=subscriber.project
        WHERE account=$1 AND project.app_domain=$2
    ";
    sqlx::query_as::<Postgres, SubscriberWithProject>(query)
        .bind(account.as_ref())
        .bind(app_domain)
        .fetch_all(postgres)
        .await
}

pub async fn upsert_subscription_watcher(
    account: AccountId,
    project: Option<Uuid>,
    did_key: &str,
    sym_key: &str,
    expiry: DateTime<Utc>,
    postgres: &PgPool,
) -> Result<(), sqlx::error::Error> {
    let _ = sqlx::query::<Postgres>(
        "
            INSERT INTO subscription_watcher (
                account,
                project,
                did_key,
                sym_key,
                expiry
            )
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (did_key) DO UPDATE SET
                sym_key = $4,
                expiry = $5
        ",
    )
    .bind(account.as_ref())
    .bind(project)
    .bind(did_key)
    .bind(sym_key)
    .bind(expiry)
    .execute(postgres)
    .await?;

    Ok(())
}

#[derive(Debug, FromRow)]
pub struct SubscriptionWatcherQuery {
    pub project: Option<Uuid>,
    pub did_key: String,
    pub sym_key: String,
}

pub async fn get_subscription_watchers_for_account_by_app_or_all_app(
    account: AccountId,
    app_domain: &str,
    postgres: &PgPool,
) -> Result<Vec<SubscriptionWatcherQuery>, sqlx::error::Error> {
    let query = "
        SELECT project, did_key, sym_key
        FROM subscription_watcher
        JOIN project ON project.id=subscription_watcher.project
        WHERE account=$1 AND (project IS NULL OR project.app_domain=$2)
    ";
    sqlx::query_as::<Postgres, SubscriptionWatcherQuery>(query)
        .bind(account.as_ref())
        .bind(app_domain)
        .fetch_all(postgres)
        .await
}

pub async fn delete_expired_subscription_watchers(
    postgres: &PgPool,
) -> Result<i64, sqlx::error::Error> {
    #[derive(Debug, FromRow)]
    struct DeleteResult {
        count: i64,
    }
    let query = "
        WITH deleted AS (
            DELETE FROM subscription_watcher
            WHERE expiry < now()
            RETURNING *
        )
        SELECT count(*) FROM deleted
    ";
    let result = sqlx::query_as::<Postgres, DeleteResult>(query)
        .fetch_one(postgres)
        .await?;
    Ok(result.count)
}
