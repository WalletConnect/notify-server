use {
    crate::{
        auth::{
            add_ttl, from_jwt, sign_jwt, verify_identity, AuthError, Authorization, AuthorizedApp,
            DidWeb, SharedClaims, SubscriptionGetNotificationsRequestAuth,
            SubscriptionGetNotificationsResponseAuth,
        },
        error::Error,
        model::helpers::{
            get_notifications_for_subscriber, get_project_by_id, get_subscriber_by_topic,
        },
        publish_relay_message::publish_relay_message,
        rate_limit::{self, Clock},
        registry::storage::redis::Redis,
        services::websocket_server::{
            decode_key, handlers::decrypt_message, AuthMessage, NotifyRequest, NotifyResponse,
        },
        spec::{
            NOTIFY_GET_NOTIFICATIONS_ACT, NOTIFY_GET_NOTIFICATIONS_RESPONSE_ACT,
            NOTIFY_GET_NOTIFICATIONS_RESPONSE_TAG, NOTIFY_GET_NOTIFICATIONS_RESPONSE_TTL,
        },
        state::AppState,
        types::{Envelope, EnvelopeType0},
        utils::topic_from_key,
        Result,
    },
    base64::Engine,
    chrono::Utc,
    relay_client::websocket::PublishedMessage,
    relay_rpc::{
        domain::{DecodedClientId, Topic},
        rpc::Publish,
    },
    std::sync::Arc,
    tracing::info,
};

// TODO test idempotency
pub async fn handle(msg: PublishedMessage, state: &AppState) -> Result<()> {
    let topic = msg.topic;

    if let Some(redis) = state.redis.as_ref() {
        notify_get_notifications_rate_limit(redis, &topic, &state.clock).await?;
    }

    // TODO combine these two SQL queries
    let subscriber =
        get_subscriber_by_topic(topic.clone(), &state.postgres, state.metrics.as_ref())
            .await
            .map_err(|e| match e {
                sqlx::Error::RowNotFound => Error::NoClientDataForTopic(topic.clone()),
                e => e.into(),
            })?;
    let project =
        get_project_by_id(subscriber.project, &state.postgres, state.metrics.as_ref()).await?;
    info!("project.id: {}", project.id);

    let envelope = Envelope::<EnvelopeType0>::from_bytes(
        base64::engine::general_purpose::STANDARD.decode(msg.message.to_string())?,
    )?;

    let sym_key = decode_key(&subscriber.sym_key)?;
    if topic != topic_from_key(&sym_key) {
        return Err(Error::TopicDoesNotMatchKey);
    }

    let msg: NotifyRequest<AuthMessage> = decrypt_message(envelope, &sym_key)?;

    let request = from_jwt::<SubscriptionGetNotificationsRequestAuth>(&msg.params.auth)?;
    info!(
        "sub_auth.shared_claims.iss: {:?}",
        request.shared_claims.iss
    );
    if request.app.domain() != project.app_domain {
        Err(Error::AppDoesNotMatch)?;
    }

    let account = {
        if request.shared_claims.act != NOTIFY_GET_NOTIFICATIONS_ACT {
            return Err(AuthError::InvalidAct)?;
        }

        let Authorization {
            account,
            app,
            domain: _,
        } = verify_identity(
            &request.shared_claims.iss,
            &request.ksu,
            &request.sub,
            state.redis.as_ref(),
            state.metrics.as_ref(),
        )
        .await?;

        // TODO verify `sub_auth.aud` matches `project_data.identity_keypair`

        if let AuthorizedApp::Limited(app) = app {
            if app != project.app_domain {
                Err(Error::AppSubscriptionsUnauthorized)?;
            }
        }

        account
    };

    request.validate()?;

    let data = get_notifications_for_subscriber(
        subscriber.id,
        request.params,
        &state.postgres,
        state.metrics.as_ref(),
    )
    .await?;

    let identity = DecodedClientId(decode_key(&project.authentication_public_key)?);

    let now = Utc::now();
    let response_message = SubscriptionGetNotificationsResponseAuth {
        shared_claims: SharedClaims {
            iat: now.timestamp() as u64,
            exp: add_ttl(now, NOTIFY_GET_NOTIFICATIONS_RESPONSE_TTL).timestamp() as u64,
            iss: identity.to_did_key(),
            aud: request.shared_claims.iss,
            act: NOTIFY_GET_NOTIFICATIONS_RESPONSE_ACT.to_owned(),
            mjv: "1".to_owned(),
        },
        sub: account.to_did_pkh(),
        app: DidWeb::from_domain(project.app_domain.clone()),
        result: data,
    };
    let auth = sign_jwt(
        response_message,
        &ed25519_dalek::SigningKey::from_bytes(&decode_key(&project.authentication_private_key)?),
    )?;

    let response = NotifyResponse::new(msg.id, AuthMessage { auth });

    let envelope = Envelope::<EnvelopeType0>::new(&sym_key, response)?;

    let response = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    publish_relay_message(
        &state.relay_http_client,
        &Publish {
            topic,
            message: response.into(),
            tag: NOTIFY_GET_NOTIFICATIONS_RESPONSE_TAG,
            ttl_secs: NOTIFY_GET_NOTIFICATIONS_RESPONSE_TTL.as_secs() as u32,
            prompt: false,
        },
        state.metrics.as_ref(),
    )
    .await?;

    Ok(())
}

pub async fn notify_get_notifications_rate_limit(
    redis: &Arc<Redis>,
    topic: &Topic,
    clock: &Clock,
) -> Result<()> {
    rate_limit::token_bucket(
        redis,
        format!("notify-get-notifications-{topic}"),
        100,
        chrono::Duration::milliseconds(500),
        1,
        clock,
    )
    .await
}
