use {
    super::types::{Project, Subscriber},
    crate::{
        auth::{
            encode_authentication_private_key, encode_authentication_public_key,
            encode_subscribe_private_key, encode_subscribe_public_key,
        },
        metrics::Metrics,
        model::types::AccountId,
    },
    chrono::{DateTime, Utc},
    ed25519_dalek::SigningKey,
    relay_rpc::domain::{ProjectId, Topic},
    serde::{Deserialize, Serialize},
    sqlx::{FromRow, PgPool, Postgres},
    std::{collections::HashSet, time::Instant},
    tracing::instrument,
    uuid::Uuid,
    x25519_dalek::StaticSecret,
};

#[derive(Debug, FromRow)]
pub struct ProjectWithPublicKeys {
    pub authentication_public_key: String,
    pub subscribe_public_key: String,
}

pub async fn upsert_project(
    project_id: ProjectId,
    app_domain: &str,
    topic: Topic,
    authentication_key: &SigningKey,
    subscribe_key: &StaticSecret,
    postgres: &PgPool,
    metrics: Option<&Metrics>,
) -> Result<ProjectWithPublicKeys, sqlx::error::Error> {
    let authentication_public_key = encode_authentication_public_key(authentication_key);
    let authentication_private_key = encode_authentication_private_key(authentication_key);
    let subscribe_public_key = encode_subscribe_public_key(subscribe_key);
    let subscribe_private_key = encode_subscribe_private_key(subscribe_key);
    upsert_project_impl(
        project_id,
        app_domain,
        topic,
        authentication_public_key,
        authentication_private_key,
        subscribe_public_key,
        subscribe_private_key,
        postgres,
        metrics,
    )
    .await
}

// TODO test idempotency
#[allow(clippy::too_many_arguments)]
#[instrument(skip(authentication_private_key, subscribe_private_key, postgres, metrics))]
async fn upsert_project_impl(
    project_id: ProjectId,
    app_domain: &str,
    topic: Topic,
    authentication_public_key: String,
    authentication_private_key: String,
    subscribe_public_key: String,
    subscribe_private_key: String,
    postgres: &PgPool,
    metrics: Option<&Metrics>,
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
            updated_at=now(),
            app_domain=$2
        RETURNING authentication_public_key, subscribe_public_key
    ";
    let start = Instant::now();
    let result = sqlx::query_as::<Postgres, ProjectWithPublicKeys>(query)
        .bind(project_id.as_ref())
        .bind(app_domain)
        .bind(topic.as_ref())
        .bind(authentication_public_key)
        .bind(authentication_private_key)
        .bind(subscribe_public_key)
        .bind(subscribe_private_key)
        .fetch_one(postgres)
        .await;
    if let Some(metrics) = metrics {
        metrics.postgres_query("upsert_project_impl", start);
    }
    result
}

#[instrument(skip(postgres, metrics))]
pub async fn get_project_by_id(
    id: Uuid,
    postgres: &PgPool,
    metrics: Option<&Metrics>,
) -> Result<Project, sqlx::error::Error> {
    let query = "
        SELECT *
        FROM project
        WHERE id=$1
    ";
    let start = Instant::now();
    let result = sqlx::query_as::<Postgres, Project>(query)
        .bind(id)
        .fetch_one(postgres)
        .await;
    if let Some(metrics) = metrics {
        metrics.postgres_query("get_project_by_id", start);
    }
    result
}

#[instrument(skip(postgres, metrics))]
pub async fn get_project_by_project_id(
    project_id: ProjectId,
    postgres: &PgPool,
    metrics: Option<&Metrics>,
) -> Result<Project, sqlx::error::Error> {
    let query = "
        SELECT *
        FROM project
        WHERE project_id=$1
    ";
    let start = Instant::now();
    let result = sqlx::query_as::<Postgres, Project>(query)
        .bind(project_id.as_ref())
        .fetch_one(postgres)
        .await;
    if let Some(metrics) = metrics {
        metrics.postgres_query("get_project_by_project_id", start);
    }
    result
}

#[instrument(skip(postgres, metrics))]
pub async fn get_project_by_app_domain(
    app_domain: &str,
    postgres: &PgPool,
    metrics: Option<&Metrics>,
) -> Result<Project, sqlx::error::Error> {
    let query = "
        SELECT *
        FROM project
        WHERE app_domain=$1
    ";
    let start = Instant::now();
    let result = sqlx::query_as::<Postgres, Project>(query)
        .bind(app_domain)
        .fetch_one(postgres)
        .await;
    if let Some(metrics) = metrics {
        metrics.postgres_query("get_project_by_app_domain", start);
    }
    result
}

#[instrument(skip(postgres, metrics))]
pub async fn get_project_by_topic(
    topic: Topic,
    postgres: &PgPool,
    metrics: Option<&Metrics>,
) -> Result<Project, sqlx::error::Error> {
    let query = "
        SELECT *
        FROM project
        WHERE topic=$1
    ";
    let start = Instant::now();
    let result = sqlx::query_as::<Postgres, Project>(query)
        .bind(topic.as_ref())
        .fetch_one(postgres)
        .await;
    if let Some(metrics) = metrics {
        metrics.postgres_query("get_project_by_topic", start);
    }
    result
}

// FIXME scaling: response not paginated
#[instrument(skip(postgres, metrics))]
pub async fn get_subscriber_accounts_by_project_id(
    project_id: ProjectId,
    postgres: &PgPool,
    metrics: Option<&Metrics>,
) -> Result<Vec<AccountId>, sqlx::error::Error> {
    #[derive(Debug, FromRow)]
    struct SubscriberAccount {
        #[sqlx(try_from = "String")]
        account: AccountId,
    }
    let query = "
        SELECT account
        FROM subscriber
        JOIN project ON project.id=subscriber.project
        WHERE project.project_id=$1
    ";
    let start = Instant::now();
    let subscribers = sqlx::query_as::<Postgres, SubscriberAccount>(query)
        .bind(project_id.as_ref())
        .fetch_all(postgres)
        .await?;
    if let Some(metrics) = metrics {
        metrics.postgres_query("get_subscriber_accounts_by_project_id", start);
    }
    Ok(subscribers.into_iter().map(|p| p.account).collect())
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct SubscriberAccountAndScopes {
    pub account: AccountId,
    pub scope: HashSet<Uuid>,
}

// FIXME scaling: response not paginated
#[instrument(skip(postgres, metrics))]
pub async fn get_subscriber_accounts_and_scopes_by_project_id(
    project_id: ProjectId,
    postgres: &PgPool,
    metrics: Option<&Metrics>,
) -> Result<Vec<SubscriberAccountAndScopes>, sqlx::error::Error> {
    #[derive(Debug, FromRow)]
    struct ResultSubscriberAccountAndScopes {
        #[sqlx(try_from = "String")]
        account: AccountId,
        scope: Vec<String>,
    }
    let query = "
        SELECT account, array_agg(subscriber_scope.name) as scope
        FROM subscriber
        JOIN project ON project.id=subscriber.project
        JOIN subscriber_scope ON subscriber_scope.subscriber=subscriber.id
        WHERE project.project_id=$1
        GROUP BY account
    ";
    let start = Instant::now();
    let projects = sqlx::query_as::<Postgres, ResultSubscriberAccountAndScopes>(query)
        .bind(project_id.as_ref())
        .fetch_all(postgres)
        .await?;
    if let Some(metrics) = metrics {
        metrics.postgres_query("get_subscriber_accounts_and_scopes_by_project_id", start);
    }
    Ok(projects
        .into_iter()
        .map(|s| SubscriberAccountAndScopes {
            account: s.account,
            scope: parse_scopes_and_ignore_invalid(&s.scope),
        })
        .collect())
}

// FIXME scaling: response not paginated
#[instrument(skip(postgres, metrics))]
pub async fn get_subscriber_topics(
    postgres: &PgPool,
    metrics: Option<&Metrics>,
) -> Result<Vec<Topic>, sqlx::error::Error> {
    #[derive(Debug, FromRow)]
    struct SubscriberWithTopic {
        #[sqlx(try_from = "String")]
        topic: Topic,
    }
    let query = "
        SELECT topic
        FROM subscriber
    ";
    let start = Instant::now();
    let subscribers = sqlx::query_as::<Postgres, SubscriberWithTopic>(query)
        .fetch_all(postgres)
        .await?;
    if let Some(metrics) = metrics {
        metrics.postgres_query("get_subscriber_topics", start);
    }
    Ok(subscribers.into_iter().map(|p| p.topic).collect())
}

// FIXME scaling: response not paginated
#[instrument(skip(postgres, metrics))]
pub async fn get_project_topics(
    postgres: &PgPool,
    metrics: Option<&Metrics>,
) -> Result<Vec<Topic>, sqlx::error::Error> {
    #[derive(Debug, FromRow)]
    struct ProjectWithTopic {
        #[sqlx(try_from = "String")]
        topic: Topic,
    }
    let query = "
        SELECT topic
        FROM project
    ";
    let start = Instant::now();
    let projects = sqlx::query_as::<Postgres, ProjectWithTopic>(query)
        .fetch_all(postgres)
        .await?;
    if let Some(metrics) = metrics {
        metrics.postgres_query("get_project_topics", start);
    }
    Ok(projects.into_iter().map(|p| p.topic).collect())
}

// TODO test idempotency
#[instrument(skip(postgres, metrics))]
pub async fn upsert_subscriber(
    project: Uuid,
    account: AccountId,
    scope: HashSet<Uuid>,
    notify_key: &[u8; 32],
    notify_topic: Topic,
    postgres: &PgPool,
    metrics: Option<&Metrics>,
) -> Result<Uuid, sqlx::error::Error> {
    let mut txn = postgres.begin().await?;

    // Note that sym_key and topic are updated on conflict. This could be implemented return the existing value like subscribe-topic does,
    // but no reason to currently: https://walletconnect.slack.com/archives/C044SKFKELR/p1701994415291179?thread_ts=1701960403.729959&cid=C044SKFKELR

    #[derive(Debug, FromRow)]
    struct SubscriberWithId {
        id: Uuid,
    }
    let query = "
        INSERT INTO subscriber (
            project,
            account,
            sym_key,
            topic,
            expiry
        )
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (project, account) DO UPDATE SET
            updated_at=now(),
            sym_key=$3,
            topic=$4,
            expiry=$5
        RETURNING id
    ";
    let start = Instant::now();
    let subscriber = sqlx::query_as::<Postgres, SubscriberWithId>(query)
        .bind(project)
        .bind(account.as_ref())
        .bind(hex::encode(notify_key))
        .bind(notify_topic.as_ref())
        .bind(Utc::now() + chrono::Duration::days(30))
        .fetch_one(&mut *txn)
        .await?;
    if let Some(metrics) = metrics {
        metrics.postgres_query("upsert_subscriber", start);
    }

    update_subscriber_scope(subscriber.id, scope, &mut txn, metrics).await?;

    txn.commit().await?;

    Ok(subscriber.id)
}

// TODO test idempotency
#[instrument(skip(postgres, metrics))]
pub async fn update_subscriber(
    project: Uuid,
    account: AccountId,
    scope: HashSet<Uuid>,
    postgres: &PgPool,
    metrics: Option<&Metrics>,
) -> Result<Subscriber, sqlx::error::Error> {
    let mut txn = postgres.begin().await?;

    let query = "
        UPDATE subscriber
        SET updated_at=now(),
            expiry=$1
        WHERE project=$2 AND account=$3
        RETURNING *
    ";
    let start = Instant::now();
    let updated_subscriber = sqlx::query_as::<_, Subscriber>(query)
        .bind(Utc::now() + chrono::Duration::days(30))
        .bind(project)
        .bind(account.as_ref())
        .fetch_one(&mut *txn)
        .await?;
    if let Some(metrics) = metrics {
        metrics.postgres_query("update_subscriber", start);
    }

    update_subscriber_scope(updated_subscriber.id, scope, &mut txn, metrics).await?;

    txn.commit().await?;

    Ok(updated_subscriber)
}

// TODO limit to 15 scopes
async fn update_subscriber_scope(
    subscriber: Uuid,
    scope: HashSet<Uuid>,
    txn: &mut sqlx::Transaction<'_, Postgres>,
    metrics: Option<&Metrics>,
) -> Result<(), sqlx::error::Error> {
    let query = "
        DELETE FROM subscriber_scope
        WHERE subscriber=$1
    ";
    let start = Instant::now();
    sqlx::query(query)
        .bind(subscriber)
        .execute(&mut **txn)
        .await?;
    if let Some(metrics) = metrics {
        metrics.postgres_query("update_subscriber_scope.delete", start);
    }

    let query = "
        INSERT INTO subscriber_scope ( subscriber, name )
        SELECT $1 AS subscriber, name FROM UNNEST($2) AS name;
    ";
    let start = Instant::now();
    let _ = sqlx::query::<Postgres>(query)
        .bind(subscriber)
        .bind(scope.into_iter().collect::<Vec<_>>())
        .execute(&mut **txn)
        .await?;
    if let Some(metrics) = metrics {
        metrics.postgres_query("update_subscriber_scope.insert", start);
    }

    Ok(())
}

#[instrument(skip(postgres, metrics))]
pub async fn delete_subscriber(
    subscriber: Uuid,
    postgres: &PgPool,
    metrics: Option<&Metrics>,
) -> Result<(), sqlx::error::Error> {
    let query = "
        DELETE FROM subscriber
        WHERE id=$1
    ";
    let start = Instant::now();
    let _ = sqlx::query::<Postgres>(query)
        .bind(subscriber)
        .execute(postgres)
        .await?;
    if let Some(metrics) = metrics {
        metrics.postgres_query("delete_subscriber", start);
    }
    Ok(())
}

pub struct SubscriberWithScope {
    pub id: Uuid,
    pub project: Uuid,
    pub account: AccountId,
    pub sym_key: String,
    pub topic: Topic,
    pub scope: HashSet<Uuid>,
    pub expiry: DateTime<Utc>,
}

#[derive(FromRow)]
pub struct SubscriberWithScopeResult {
    pub id: Uuid,
    pub project: Uuid,
    #[sqlx(try_from = "String")]
    pub account: AccountId,
    pub sym_key: String,
    #[sqlx(try_from = "String")]
    pub topic: Topic,
    pub scope: Vec<String>,
    pub expiry: DateTime<Utc>,
}

impl From<SubscriberWithScopeResult> for SubscriberWithScope {
    fn from(val: SubscriberWithScopeResult) -> Self {
        SubscriberWithScope {
            id: val.id,
            project: val.project,
            account: val.account,
            sym_key: val.sym_key,
            topic: val.topic,
            scope: parse_scopes_and_ignore_invalid(&val.scope),
            expiry: val.expiry,
        }
    }
}

#[instrument(skip(postgres, metrics))]
pub async fn get_subscriber_by_topic(
    topic: Topic,
    postgres: &PgPool,
    metrics: Option<&Metrics>,
) -> Result<SubscriberWithScope, sqlx::error::Error> {
    let query = "
        SELECT subscriber.id, project, account, sym_key, array_agg(subscriber_scope.name) as \
                 scope, topic, expiry
        FROM subscriber
        JOIN subscriber_scope ON subscriber_scope.subscriber=subscriber.id
        WHERE topic=$1
        GROUP BY subscriber.id, project, account, sym_key, topic, expiry
    ";
    let start = Instant::now();
    let result = sqlx::query_as::<Postgres, SubscriberWithScopeResult>(query)
        .bind(topic.as_ref())
        .fetch_one(postgres)
        .await
        .map(Into::into);
    if let Some(metrics) = metrics {
        metrics.postgres_query("get_subscriber_by_topic", start);
    }
    result
}

pub struct NotifySubscriberInfo {
    pub id: Uuid,
    pub account: AccountId,
    pub scope: HashSet<Uuid>,
}

#[derive(FromRow)]
pub struct NotifySubscriberInfoResult {
    pub id: Uuid,
    #[sqlx(try_from = "String")]
    pub account: AccountId,
    pub scope: Vec<String>,
}

impl From<NotifySubscriberInfoResult> for NotifySubscriberInfo {
    fn from(val: NotifySubscriberInfoResult) -> Self {
        NotifySubscriberInfo {
            id: val.id,
            account: val.account,
            scope: parse_scopes_and_ignore_invalid(&val.scope),
        }
    }
}

#[instrument(skip(postgres, metrics))]
pub async fn get_subscribers_for_project_in(
    project: Uuid,
    accounts: &[AccountId],
    postgres: &PgPool,
    metrics: Option<&Metrics>,
) -> Result<Vec<NotifySubscriberInfo>, sqlx::error::Error> {
    let query = "
        SELECT subscriber.id, account, array_agg(subscriber_scope.name) as scope
        FROM subscriber
        JOIN subscriber_scope ON subscriber_scope.subscriber=subscriber.id
        WHERE project=$1 AND account = ANY($2)
        GROUP BY subscriber.id, project, account, sym_key, topic, expiry
    ";
    let start = Instant::now();
    let result = sqlx::query_as::<Postgres, NotifySubscriberInfoResult>(query)
        .bind(project)
        .bind(accounts.iter().map(|a| a.as_ref()).collect::<Vec<_>>())
        .fetch_all(postgres)
        .await
        .map(|vec| vec.into_iter().map(Into::into).collect());
    if let Some(metrics) = metrics {
        metrics.postgres_query("get_subscribers_for_project_in", start);
    }
    result
}

pub struct SubscriberWithProject {
    /// App domain that the subscription refers to
    pub app_domain: String,
    /// Authentication key used for authenticating topic JWTs and setting JWT aud field
    pub authentication_public_key: String,
    /// CAIP-10 account
    pub account: AccountId, // TODO do we need to return this?
    /// Symetric key used for notify topic. sha256 to get notify topic to manage
    /// the subscription and call wc_notifySubscriptionUpdate and
    /// wc_notifySubscriptionDelete
    pub sym_key: String,
    /// Array of notification types enabled for this subscription
    pub scope: HashSet<Uuid>,
    /// Unix timestamp of expiration
    pub expiry: DateTime<Utc>,
}

#[derive(FromRow)]
struct SubscriberWithProjectResult {
    pub app_domain: String,
    pub authentication_public_key: String,
    #[sqlx(try_from = "String")]
    pub account: AccountId,
    pub sym_key: String,
    pub scope: Vec<String>,
    pub expiry: DateTime<Utc>,
}

impl From<SubscriberWithProjectResult> for SubscriberWithProject {
    fn from(val: SubscriberWithProjectResult) -> Self {
        SubscriberWithProject {
            app_domain: val.app_domain,
            authentication_public_key: val.authentication_public_key,
            account: val.account,
            sym_key: val.sym_key,
            scope: parse_scopes_and_ignore_invalid(&val.scope),
            expiry: val.expiry,
        }
    }
}

fn parse_scopes_and_ignore_invalid(scopes: &[String]) -> HashSet<Uuid> {
    scopes
        .iter()
        .filter_map(|s| Uuid::parse_str(s).ok())
        .collect()
}

// TODO this doesn't need to return a full subscriber (especially not scopes)
#[instrument(skip(postgres, metrics))]
pub async fn get_subscriptions_by_account(
    account: AccountId,
    postgres: &PgPool,
    metrics: Option<&Metrics>,
) -> Result<Vec<SubscriberWithProject>, sqlx::error::Error> {
    let query: &str = "
        SELECT app_domain, project.authentication_public_key, account, sym_key, array_agg(subscriber_scope.name) as scope, expiry
        FROM subscriber
        JOIN project ON project.id=subscriber.project
        JOIN subscriber_scope ON subscriber_scope.subscriber=subscriber.id
        WHERE account=$1
        GROUP BY app_domain, project.authentication_public_key, account, sym_key, expiry
    ";
    let start = Instant::now();
    let result = sqlx::query_as::<Postgres, SubscriberWithProjectResult>(query)
        .bind(account.as_ref())
        .fetch_all(postgres)
        .await
        .map(|result| result.into_iter().map(Into::into).collect());
    if let Some(metrics) = metrics {
        metrics.postgres_query("get_subscriptions_by_account", start);
    }

    result
}

// TODO this doesn't need to return a full subscriber (especially not scopes)
#[instrument(skip(postgres, metrics))]
pub async fn get_subscriptions_by_account_and_app(
    account: AccountId,
    app_domain: &str,
    postgres: &PgPool,
    metrics: Option<&Metrics>,
) -> Result<Vec<SubscriberWithProject>, sqlx::error::Error> {
    let query: &str = "
        SELECT app_domain, project.authentication_public_key, sym_key, account, array_agg(subscriber_scope.name) as scope, expiry
        FROM subscriber
        JOIN project ON project.id=subscriber.project
        JOIN subscriber_scope ON subscriber_scope.subscriber=subscriber.id
        WHERE account=$1 AND project.app_domain=$2
        GROUP BY app_domain, project.authentication_public_key, sym_key, account, expiry
    ";
    let start = Instant::now();
    let result = sqlx::query_as::<Postgres, SubscriberWithProjectResult>(query)
        .bind(account.as_ref())
        .bind(app_domain)
        .fetch_all(postgres)
        .await
        .map(|result| result.into_iter().map(Into::into).collect());
    if let Some(metrics) = metrics {
        metrics.postgres_query("get_subscriptions_by_account_and_app", start);
    }
    result
}

#[instrument(skip(postgres, metrics))]
pub async fn upsert_subscription_watcher(
    account: AccountId,
    project: Option<Uuid>,
    did_key: &str,
    sym_key: &str,
    expiry: DateTime<Utc>,
    postgres: &PgPool,
    metrics: Option<&Metrics>,
) -> Result<(), sqlx::error::Error> {
    let start = Instant::now();
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
                updated_at=now(),
                account=$1,
                project=$2,
                sym_key=$4,
                expiry=$5
        ",
    )
    .bind(account.as_ref())
    .bind(project)
    .bind(did_key)
    .bind(sym_key)
    .bind(expiry)
    .execute(postgres)
    .await?;
    if let Some(metrics) = metrics {
        metrics.postgres_query("upsert_subscription_watcher", start);
    }

    Ok(())
}

#[derive(Debug, FromRow)]
pub struct SubscriptionWatcherQuery {
    pub project: Option<Uuid>,
    pub did_key: String,
    pub sym_key: String,
}

#[instrument(skip(postgres, metrics))]
pub async fn get_subscription_watchers_for_account_by_app_or_all_app(
    account: AccountId,
    app_domain: &str,
    postgres: &PgPool,
    metrics: Option<&Metrics>,
) -> Result<Vec<SubscriptionWatcherQuery>, sqlx::error::Error> {
    let query = "
        SELECT project, did_key, sym_key
        FROM subscription_watcher
        LEFT JOIN project ON project.id=subscription_watcher.project
        WHERE expiry > now() AND account=$1 AND (project IS NULL OR project.app_domain=$2)
    ";
    let start = Instant::now();
    let result = sqlx::query_as::<Postgres, SubscriptionWatcherQuery>(query)
        .bind(account.as_ref())
        .bind(app_domain)
        .fetch_all(postgres)
        .await;
    if let Some(metrics) = metrics {
        metrics.postgres_query(
            "get_subscription_watchers_for_account_by_app_or_all_app",
            start,
        );
    }
    result
}

#[instrument(skip(postgres, metrics))]
pub async fn delete_expired_subscription_watchers(
    postgres: &PgPool,
    metrics: Option<&Metrics>,
) -> Result<i64, sqlx::error::Error> {
    #[derive(Debug, FromRow)]
    struct DeleteResult {
        count: i64,
    }
    let query = "
        WITH deleted AS (
            DELETE FROM subscription_watcher
            WHERE expiry <= now()
            RETURNING *
        )
        SELECT count(*) FROM deleted
    ";
    let start = Instant::now();
    let result = sqlx::query_as::<Postgres, DeleteResult>(query)
        .fetch_one(postgres)
        .await?;
    if let Some(metrics) = metrics {
        metrics.postgres_query("delete_expired_subscription_watchers", start);
    }

    Ok(result.count)
}
