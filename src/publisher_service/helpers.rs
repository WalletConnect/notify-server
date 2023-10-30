use {
    super::types::{
        JwtMessage, Notification, NotificationStates, ProjectSigningDetails, Subscriber,
    },
    crate::{auth::add_ttl, model::types::AccountId, spec::NOTIFY_MESSAGE_TTL},
    base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine},
    ed25519_dalek::Signer,
    relay_rpc::jwt::{JwtHeader, JWT_HEADER_ALG, JWT_HEADER_TYP},
    sqlx::{PgPool, Postgres},
    tracing::instrument,
};

#[instrument]
pub fn sign_message(
    msg: Notification,
    account: AccountId,
    ProjectSigningDetails {
        identity,
        private_key,
        app,
    }: &ProjectSigningDetails,
) -> crate::error::Result<String> {
    let now = chrono::Utc::now();
    let message = URL_SAFE_NO_PAD.encode(serde_json::to_string(&JwtMessage {
        iat: now.timestamp(),
        exp: add_ttl(now, NOTIFY_MESSAGE_TTL).timestamp(),
        iss: format!("did:key:{identity}"),
        act: "notify_message".to_string(),
        sub: format!("did:pkh:{account}"),
        app: app.clone().to_string(),
        msg,
    })?);

    let header = URL_SAFE_NO_PAD.encode(serde_json::to_string(&JwtHeader {
        typ: JWT_HEADER_TYP,
        alg: JWT_HEADER_ALG,
    })?);

    let message = format!("{header}.{message}");
    let signature = private_key.sign(message.as_bytes());
    let signature = URL_SAFE_NO_PAD.encode(signature.to_bytes());

    Ok(format!("{message}.{signature}"))
}

#[instrument(skip(pg_pool))]
pub async fn get_notification_by_notification_id(
    notification_id: &str,
    pg_pool: &PgPool,
) -> Result<Notification, sqlx::error::Error> {
    let query = "
      SELECT *
      FROM project
      WHERE project_id=$1
  ";
    sqlx::query_as::<Postgres, Notification>(query)
        .bind(notification_id)
        .fetch_one(pg_pool)
        .await
}

#[instrument(skip(pg_pool))]
pub async fn get_subscriber_by_subscriber_id(
    subscriber_id: &str,
    pg_pool: &PgPool,
) -> Result<Subscriber, sqlx::error::Error> {
    let query = "
      SELECT *
      FROM subscriber
      WHERE project_id=$1
  ";
    sqlx::query_as::<Postgres, Subscriber>(query)
        .bind(subscriber_id)
        .fetch_one(pg_pool)
        .await
}

#[instrument(skip(pg_pool))]
pub async fn update_message_processing_status(
    message_id: &str,
    status: NotificationStates,
    pg_pool: &PgPool,
) -> std::result::Result<(), sqlx::error::Error> {
    let mark_message_as_processed = "
        UPDATE subscriber_notification SET
            status=$1, 
            updated_at=NOW()
        WHERE id=$2;
    ";
    sqlx::query::<Postgres>(mark_message_as_processed)
        .bind(status.to_string())
        .bind(message_id)
        .execute(pg_pool)
        .await?;
    Ok(())
}
