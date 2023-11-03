use {
    super::types::SubscriberNotificationStatus,
    crate::{model::types::AccountId, types::Notification},
    relay_rpc::domain::{ProjectId, Topic},
    sqlx::{FromRow, PgPool, Postgres},
    tracing::instrument,
    uuid::Uuid,
};

#[derive(Debug, FromRow)]
pub struct NotificationWithId {
    pub id: Uuid,
    pub notification: Notification,
}

#[instrument(skip(postgres))]
pub async fn upsert_notification(
    notification_id: String,
    project: Uuid,
    notification: Notification,
    postgres: &PgPool,
) -> Result<NotificationWithId, sqlx::Error> {
    #[derive(Debug, FromRow)]
    pub struct Result {
        pub id: Uuid,
        pub r#type: String,
        pub title: String,
        pub body: String,
        pub icon: Option<String>,
        pub url: Option<String>,
    }
    let query = "
        INSERT INTO notification (project, notification_id, type, title, body, icon, url)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        ON CONFLICT (project, notification_id) DO NOTHING
        RETURNING id, type, title, body, icon, url
    ";
    let result = sqlx::query_as::<Postgres, Result>(query)
        .bind(project)
        .bind(notification_id)
        .bind(notification.r#type)
        .bind(notification.title)
        .bind(notification.body)
        .bind(notification.icon)
        .bind(notification.url)
        .fetch_one(postgres)
        .await?;
    Ok(NotificationWithId {
        id: result.id,
        notification: Notification {
            r#type: result.r#type,
            title: result.title,
            body: result.body,
            icon: result.icon,
            url: result.url,
        },
    })
}

pub async fn upsert_subcriber_notification(
    notification: Uuid,
    subscriber: Uuid,
    postgres: &PgPool,
) -> Result<(), sqlx::Error> {
    let query = "
        INSERT INTO subscriber_notification (notification, subscriber, status)
        VALUES ($1, $2, $3::subscriber_notification_status)
        ON CONFLICT (notification, subscriber) DO NOTHING
    ";
    sqlx::query(query)
        .bind(notification)
        .bind(subscriber)
        .bind(SubscriberNotificationStatus::Queued.to_string())
        .execute(postgres)
        .await?;
    Ok(())
}

#[derive(Debug, FromRow)]
pub struct NotificationToProcess {
    pub id: Uuid,
    pub notification_type: String,
    pub notification_title: String,
    pub notification_body: String,
    pub notification_icon: Option<String>,
    pub notification_url: Option<String>,
    pub subscriber: Uuid,
    #[sqlx(try_from = "String")]
    pub subscriber_account: AccountId,
    pub subscriber_sym_key: String,
    #[sqlx(try_from = "String")]
    pub subscriber_topic: Topic,
    pub project: Uuid,
    #[sqlx(try_from = "String")]
    pub project_project_id: ProjectId,
    pub project_app_domain: String,
    pub project_authentication_public_key: String,
    pub project_authentication_private_key: String,
}

#[instrument(skip(postgres))]
pub async fn pick_subscriber_notification_for_processing(
    postgres: &PgPool,
) -> Result<Option<NotificationToProcess>, sqlx::Error> {
    // Getting the notification to be published from the `subscriber_notification`,
    // updating the status to the `processing`,
    // and returning the notification to be processed
    let mut txn = postgres.begin().await?;

    let query = "
        SELECT
            subscriber_notification.id AS id,
            notification.type AS notification_type,
            notification.title AS notification_title,
            notification.body AS notification_body,
            notification.icon AS notification_icon,
            notification.url AS notification_url,
            subscriber.id AS subscriber,
            subscriber.account AS subscriber_account,
            subscriber.sym_key AS subscriber_sym_key,
            subscriber.topic AS subscriber_topic,
            project.id AS project,
            project.project_id AS project_project_id,
            project.app_domain AS project_app_domain,
            project.authentication_public_key AS project_authentication_public_key,
            project.authentication_private_key AS project_authentication_private_key
        FROM subscriber_notification
        JOIN notification ON notification.id=subscriber_notification.notification
        JOIN subscriber ON subscriber.id=subscriber_notification.subscriber
        JOIN project ON project.id=notification.project
        WHERE subscriber_notification.status='queued'
        LIMIT 1
        FOR UPDATE SKIP LOCKED
    ";
    let notification = sqlx::query_as::<Postgres, NotificationToProcess>(query)
        .fetch_optional(&mut *txn)
        .await?;

    if let Some(notification) = &notification {
        let query = "
        UPDATE subscriber_notification
        SET updated_at=now(),
            status='processing'
        WHERE id=$1
    ";
        sqlx::query::<Postgres>(query)
            .bind(notification.id)
            .execute(&mut *txn)
            .await?;
    }

    txn.commit().await?;

    Ok(notification)
}

#[instrument(skip(postgres))]
pub async fn update_message_processing_status(
    notification: Uuid,
    status: SubscriberNotificationStatus,
    postgres: &PgPool,
) -> std::result::Result<(), sqlx::error::Error> {
    let mark_message_as_processed = "
        UPDATE subscriber_notification
        SET updated_at=now(),
            status=$1::subscriber_notification_status
        WHERE id=$2;
    ";
    sqlx::query::<Postgres>(mark_message_as_processed)
        .bind(status.to_string())
        .bind(notification)
        .execute(postgres)
        .await?;
    Ok(())
}
