use {
    super::notify_watch_subscriptions::update_subscription_watchers,
    crate::{
        analytics::subscriber_update::{NotifyClientMethod, SubscriberUpdateParams},
        auth::{
            add_ttl, from_jwt, sign_jwt, verify_identity, AuthError, Authorization, AuthorizedApp,
            DidWeb, SharedClaims, SubscriptionUpdateRequestAuth, SubscriptionUpdateResponseAuth,
        },
        error::Error,
        model::helpers::{get_project_by_id, get_subscriber_by_topic, update_subscriber},
        publish_relay_message::publish_relay_message,
        rate_limit::{self, Clock},
        registry::storage::redis::Redis,
        services::websocket_server::{
            decode_key, handlers::decrypt_message, NotifyRequest, NotifyResponse, NotifyUpdate,
            ResponseAuth,
        },
        spec::{
            NOTIFY_UPDATE_ACT, NOTIFY_UPDATE_RESPONSE_ACT, NOTIFY_UPDATE_RESPONSE_TAG,
            NOTIFY_UPDATE_RESPONSE_TTL,
        },
        state::AppState,
        types::{parse_scope, Envelope, EnvelopeType0},
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
    std::{collections::HashSet, sync::Arc},
    tracing::info,
};

// TODO test idempotency
pub async fn handle(msg: PublishedMessage, state: &AppState) -> Result<()> {
    let topic = msg.topic;

    if let Some(redis) = state.redis.as_ref() {
        notify_update_rate_limit(redis, &topic, &state.clock).await?;
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

    let msg: NotifyRequest<NotifyUpdate> = decrypt_message(envelope, &sym_key)?;

    let sub_auth = from_jwt::<SubscriptionUpdateRequestAuth>(&msg.params.update_auth)?;
    info!(
        "sub_auth.shared_claims.iss: {:?}",
        sub_auth.shared_claims.iss
    );
    if sub_auth.app.domain() != project.app_domain {
        Err(Error::AppDoesNotMatch)?;
    }

    let (account, siwe_domain) = {
        if sub_auth.shared_claims.act != NOTIFY_UPDATE_ACT {
            return Err(AuthError::InvalidAct)?;
        }

        let Authorization {
            account,
            app,
            domain,
        } = verify_identity(
            &sub_auth.shared_claims.iss,
            &sub_auth.ksu,
            &sub_auth.sub,
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

        (account, domain)
    };

    let old_scope = subscriber.scope.into_iter().collect::<HashSet<_>>();
    let new_scope = parse_scope(&sub_auth.scp)?;

    let subscriber = update_subscriber(
        project.id,
        account.clone(),
        new_scope.clone(),
        &state.postgres,
        state.metrics.as_ref(),
    )
    .await?;

    // TODO do in same transaction as update_subscriber()
    // state
    //     .notify_webhook(
    //         project_id.as_ref(),
    //         WebhookNotificationEvent::Updated,
    //         account.as_ref(),
    //     )
    //     .await?;

    state.analytics.client(SubscriberUpdateParams {
        project_pk: project.id,
        project_id: project.project_id,
        pk: subscriber.id,
        account: account.clone(),
        updated_by_iss: sub_auth.shared_claims.iss.clone().into(),
        updated_by_domain: siwe_domain,
        method: NotifyClientMethod::Update,
        old_scope,
        new_scope,
        notification_topic: subscriber.topic.clone(),
        topic,
    });

    let identity = DecodedClientId(decode_key(&project.authentication_public_key)?);

    let now = Utc::now();
    let response_message = SubscriptionUpdateResponseAuth {
        shared_claims: SharedClaims {
            iat: now.timestamp() as u64,
            exp: add_ttl(now, NOTIFY_UPDATE_RESPONSE_TTL).timestamp() as u64,
            iss: identity.to_did_key(),
            aud: sub_auth.shared_claims.iss,
            act: NOTIFY_UPDATE_RESPONSE_ACT.to_owned(),
            mjv: "1".to_owned(),
        },
        sub: account.to_did_pkh(),
        app: DidWeb::from_domain(project.app_domain.clone()),
        sbs: vec![],
    };
    let response_auth = sign_jwt(
        response_message,
        &ed25519_dalek::SigningKey::from_bytes(&decode_key(&project.authentication_private_key)?),
    )?;

    let response = NotifyResponse::new(msg.id, ResponseAuth { response_auth });

    let envelope = Envelope::<EnvelopeType0>::new(&sym_key, response)?;

    let base64_notification = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    let response_topic = topic_from_key(&sym_key);

    publish_relay_message(
        &state.relay_http_client,
        &Publish {
            topic: response_topic,
            message: base64_notification.into(),
            tag: NOTIFY_UPDATE_RESPONSE_TAG,
            ttl_secs: NOTIFY_UPDATE_RESPONSE_TTL.as_secs() as u32,
            prompt: false,
        },
        state.metrics.as_ref(),
    )
    .await?;

    update_subscription_watchers(
        account,
        &project.app_domain,
        &state.postgres,
        &state.relay_http_client.clone(),
        state.metrics.as_ref(),
        &state.notify_keys.authentication_secret,
        &state.notify_keys.authentication_public,
    )
    .await?;

    Ok(())
}

pub async fn notify_update_rate_limit(
    redis: &Arc<Redis>,
    topic: &Topic,
    clock: &Clock,
) -> Result<()> {
    rate_limit::token_bucket(
        redis,
        format!("notify-update-{topic}"),
        100,
        chrono::Duration::seconds(1),
        1,
        clock,
    )
    .await
}
