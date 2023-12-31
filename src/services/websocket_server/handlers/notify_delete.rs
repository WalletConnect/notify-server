use {
    crate::{
        analytics::subscriber_update::{NotifyClientMethod, SubscriberUpdateParams},
        auth::{
            add_ttl, from_jwt, sign_jwt, verify_identity, AuthError, Authorization, AuthorizedApp,
            DidWeb, SharedClaims, SubscriptionDeleteRequestAuth, SubscriptionDeleteResponseAuth,
        },
        error::Error,
        model::helpers::{delete_subscriber, get_project_by_id, get_subscriber_by_topic},
        publish_relay_message::publish_relay_message,
        rate_limit::{self, Clock},
        registry::storage::redis::Redis,
        services::websocket_server::{
            decode_key,
            handlers::{decrypt_message, notify_watch_subscriptions::update_subscription_watchers},
            NotifyDelete, NotifyRequest, NotifyResponse, ResponseAuth,
        },
        spec::{
            NOTIFY_DELETE_ACT, NOTIFY_DELETE_RESPONSE_ACT, NOTIFY_DELETE_RESPONSE_TAG,
            NOTIFY_DELETE_RESPONSE_TTL,
        },
        state::{AppState, WebhookNotificationEvent},
        types::{Envelope, EnvelopeType0},
        utils::topic_from_key,
        Result,
    },
    base64::Engine,
    chrono::Utc,
    relay_client::websocket::{Client, PublishedMessage},
    relay_rpc::{
        domain::{DecodedClientId, Topic},
        rpc::Publish,
    },
    std::{collections::HashSet, sync::Arc},
    tracing::{info, warn},
};

// TODO make and test idempotency
pub async fn handle(msg: PublishedMessage, state: &AppState, client: &Client) -> Result<()> {
    let topic = msg.topic;
    let subscription_id = msg.subscription_id;

    if let Some(redis) = state.redis.as_ref() {
        notify_delete_rate_limit(redis, &topic, &state.clock).await?;
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

    let msg: NotifyRequest<NotifyDelete> = decrypt_message(envelope, &sym_key)?;

    let sub_auth = from_jwt::<SubscriptionDeleteRequestAuth>(&msg.params.delete_auth)?;
    info!(
        "sub_auth.shared_claims.iss: {:?}",
        sub_auth.shared_claims.iss
    );
    if sub_auth.app.domain() != project.app_domain {
        Err(Error::AppDoesNotMatch)?;
    }

    let (account, siwe_domain) = {
        if sub_auth.shared_claims.act != NOTIFY_DELETE_ACT {
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

    delete_subscriber(subscriber.id, &state.postgres, state.metrics.as_ref()).await?;

    // TODO do in same txn as delete_subscriber()
    state
        .notify_webhook(
            project.project_id.as_ref(),
            WebhookNotificationEvent::Unsubscribed,
            subscriber.account.as_ref(),
        )
        .await?;

    if let Err(e) = client.unsubscribe(topic.clone(), subscription_id).await {
        warn!("Error unsubscribing Notify from topic: {}", e);
    };

    state.analytics.client(SubscriberUpdateParams {
        project_pk: project.id,
        project_id: project.project_id,
        pk: subscriber.id,
        account: account.clone(),
        updated_by_iss: sub_auth.shared_claims.iss.clone().into(),
        updated_by_domain: siwe_domain,
        method: NotifyClientMethod::Unsubscribe,
        old_scope: subscriber.scope.into_iter().map(Into::into).collect(),
        new_scope: HashSet::new(),
        notification_topic: subscriber.topic,
        topic,
    });

    let identity = DecodedClientId(decode_key(&project.authentication_public_key)?);

    let now = Utc::now();
    let response_message = SubscriptionDeleteResponseAuth {
        shared_claims: SharedClaims {
            iat: now.timestamp() as u64,
            exp: add_ttl(now, NOTIFY_DELETE_RESPONSE_TTL).timestamp() as u64,
            iss: identity.to_did_key(),
            aud: sub_auth.shared_claims.iss,
            act: NOTIFY_DELETE_RESPONSE_ACT.to_owned(),
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
            tag: NOTIFY_DELETE_RESPONSE_TAG,
            ttl_secs: NOTIFY_DELETE_RESPONSE_TTL.as_secs() as u32,
            prompt: false,
        },
        state.metrics.as_ref(),
    )
    .await?;

    update_subscription_watchers(
        account.clone(),
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

pub async fn notify_delete_rate_limit(
    redis: &Arc<Redis>,
    topic: &Topic,
    clock: &Clock,
) -> Result<()> {
    rate_limit::token_bucket(
        redis,
        format!("notify-delete-{topic}"),
        10,
        chrono::Duration::hours(1),
        1,
        clock,
    )
    .await
}
