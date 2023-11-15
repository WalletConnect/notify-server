use {
    super::types::{PublishingQueueStats, SubscriberNotificationStatus},
    crate::{metrics::Metrics, model::types::AccountId, types::Notification},
    relay_rpc::domain::{ProjectId, Topic},
    sqlx::{FromRow, PgPool, Postgres},
    tracing::{error, instrument},
    uuid::Uuid,
    wc::metrics::otel::Context,
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
        pub r#type: Uuid,
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

pub async fn upsert_subscriber_notifications(
    notification: Uuid,
    subscribers: &[Uuid],
    postgres: &PgPool,
) -> Result<(), sqlx::Error> {
    let query = "
        INSERT INTO subscriber_notification (notification, subscriber, status)
        SELECT $1 AS notification, subscriber, $3::subscriber_notification_status FROM UNNEST($2) AS subscriber
        ON CONFLICT (notification, subscriber) DO NOTHING
    ";
    sqlx::query(query)
        .bind(notification)
        .bind(subscribers)
        .bind(SubscriberNotificationStatus::Queued.to_string())
        .execute(postgres)
        .await?;
    Ok(())
}

#[derive(Debug, FromRow)]
pub struct NotificationToProcess {
    pub notification_type: Uuid,
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
    pub subscriber_notification: Uuid,
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
            notification.type AS notification_type,
            notification.title AS notification_title,
            notification.body AS notification_body,
            notification.icon AS notification_icon,
            notification.url AS notification_url,
            subscriber.id AS subscriber,
            subscriber.account AS subscriber_account,
            subscriber.sym_key AS subscriber_sym_key,
            subscriber.topic AS subscriber_topic,
            subscriber_notification.id AS subscriber_notification,
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
        update_message_processing_status(
            notification.subscriber_notification,
            SubscriberNotificationStatus::Processing,
            &mut *txn,
            None,
        )
        .await?;
    }

    txn.commit().await?;

    Ok(notification)
}

#[instrument(skip(postgres, metrics))]
pub async fn update_message_processing_status<'e>(
    notification: Uuid,
    status: SubscriberNotificationStatus,
    postgres: impl sqlx::PgExecutor<'e>,
    metrics: Option<&Metrics>,
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

    if let Some(metrics) = metrics {
        update_metrics_on_message_status_change(metrics, status).await;
    }

    Ok(())
}

#[instrument(skip(metrics))]
pub async fn update_metrics_on_message_status_change(
    metrics: &Metrics,
    status: SubscriberNotificationStatus,
) {
    let ctx = Context::current();
    if status == SubscriberNotificationStatus::Published {
        metrics.publishing_queue_published_count.add(&ctx, 1, &[]);
    }
    // TODO: We should add a metric for the failed state when it's implemented
}

#[instrument(skip(postgres))]
pub async fn get_publishing_queue_stats(
    postgres: &PgPool,
) -> std::result::Result<PublishingQueueStats, sqlx::error::Error> {
    let query = "
    SELECT 
        (SELECT COUNT(*) FROM subscriber_notification WHERE status = 'queued') AS queued,
        (SELECT COUNT(*) FROM subscriber_notification WHERE status = 'processing') AS processing
    ";
    let notification = sqlx::query_as::<Postgres, PublishingQueueStats>(query)
        .fetch_one(postgres)
        .await?;

    Ok(notification)
}

#[instrument(skip_all)]
pub async fn update_metrics_on_queue_stats(metrics: &Metrics, postgres: &PgPool) {
    let ctx = Context::current();
    let queue_stats = get_publishing_queue_stats(postgres).await;
    match queue_stats {
        Ok(queue_stats) => {
            metrics
                .publishing_queue_queued_size
                .observe(&ctx, queue_stats.queued as u64, &[]);
            metrics.publishing_queue_processing_size.observe(
                &ctx,
                queue_stats.processing as u64,
                &[],
            );
        }
        Err(e) => {
            error!("Error on getting publishing queue stats: {:?}", e);
        }
    }
/// Checks for messages in the `processing` state for more than threshold in minutes
/// and put it back in a `queued` state for processing
#[instrument(skip(postgres))]
pub async fn dead_letters_check(
    threshold_minutes: i8,
    postgres: &PgPool,
) -> std::result::Result<(), sqlx::error::Error> {
    let update_status_query = "
        UPDATE subscriber_notification
        SET status = 'queued'
        WHERE status = 'processing'
        AND EXTRACT(EPOCH FROM (NOW() - updated_at))/60 > $1::INTEGER
    ";
    sqlx::query::<Postgres>(update_status_query)
        .bind(threshold_minutes)
        .execute(postgres)
        .await?;
    Ok(())
}

/// Checks for message is created more than threshold in minutes
#[instrument(skip(postgres))]
pub async fn dead_letter_give_up_check(
    notification: Uuid,
    threshold_minutes: i16,
    postgres: &PgPool,
) -> std::result::Result<bool, sqlx::error::Error> {
    let query_to_check = "
        SELECT now() - created_at > interval '$1 minutes' 
        FROM subscriber_notification 
        WHERE id = $2
    ";
    let row: (bool,) = sqlx::query_as(query_to_check)
        .bind(threshold_minutes)
        .bind(notification)
        .fetch_one(postgres)
        .await?;
    Ok(row.0)
}
