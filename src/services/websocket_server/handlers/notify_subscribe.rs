use {
    crate::{
        analytics::subscriber_update::{NotifyClientMethod, SubscriberUpdateParams},
        auth::{
            add_ttl, from_jwt, sign_jwt, verify_identity, AuthError, Authorization, AuthorizedApp,
            DidWeb, SharedClaims, SubscriptionRequestAuth, SubscriptionResponseAuth,
        },
        error::Error,
        model::helpers::{get_project_by_topic, get_welcome_notification, upsert_subscriber},
        publish_relay_message::publish_relay_message,
        rate_limit::{self, Clock},
        registry::storage::redis::Redis,
        services::{
            publisher_service::helpers::{upsert_notification, upsert_subscriber_notifications},
            websocket_server::{
                decode_key, derive_key,
                handlers::{
                    decrypt_message, notify_watch_subscriptions::update_subscription_watchers,
                },
                NotifyRequest, NotifyResponse, NotifySubscribe, ResponseAuth,
            },
        },
        spec::{
            NOTIFY_NOOP_TAG, NOTIFY_NOOP_TTL, NOTIFY_SUBSCRIBE_ACT, NOTIFY_SUBSCRIBE_RESPONSE_ACT,
            NOTIFY_SUBSCRIBE_RESPONSE_TAG, NOTIFY_SUBSCRIBE_RESPONSE_TTL,
        },
        state::{AppState, WebhookNotificationEvent},
        types::{parse_scope, Envelope, EnvelopeType0, EnvelopeType1, Notification},
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
    std::{
        collections::HashSet,
        sync::{Arc, OnceLock},
    },
    tracing::{info, instrument},
    uuid::Uuid,
    x25519_dalek::{PublicKey, StaticSecret},
};

// TODO limit each subscription to 15 notification types
// TODO limit each account to max 500 subscriptions

// TODO test idempotency (create subscriber a second time for the same account)
#[instrument(name = "wc_notifySubscribe", skip_all)]
pub async fn handle(msg: PublishedMessage, state: &AppState) -> Result<()> {
    let topic = msg.topic;

    if let Some(redis) = state.redis.as_ref() {
        notify_subscribe_project_rate_limit(redis, &topic, &state.clock).await?;
    }

    let project = get_project_by_topic(topic.clone(), &state.postgres, state.metrics.as_ref())
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => Error::NoProjectDataForTopic(topic.clone()),
            e => e.into(),
        })?;
    info!("project.id: {}", project.id);

    let envelope = Envelope::<EnvelopeType1>::from_bytes(
        base64::engine::general_purpose::STANDARD.decode(msg.message.to_string())?,
    )?;

    let client_public_key = x25519_dalek::PublicKey::from(envelope.pubkey());

    if let Some(redis) = state.redis.as_ref() {
        notify_subscribe_client_rate_limit(redis, &client_public_key, &state.clock).await?;
    }

    let sym_key = derive_key(
        &client_public_key,
        &x25519_dalek::StaticSecret::from(decode_key(&project.subscribe_private_key)?),
    )?;
    let response_topic = topic_from_key(&sym_key);
    info!("response_topic: {response_topic}");

    let msg: NotifyRequest<NotifySubscribe> = decrypt_message(envelope, &sym_key)?;
    let id = msg.id;

    let sub_auth = from_jwt::<SubscriptionRequestAuth>(&msg.params.subscription_auth)?;
    info!(
        "sub_auth.shared_claims.iss: {:?}",
        sub_auth.shared_claims.iss
    );
    if sub_auth.app.domain() != project.app_domain {
        Err(Error::AppDoesNotMatch)?;
    }

    let (account, siwe_domain) = {
        if sub_auth.shared_claims.act != NOTIFY_SUBSCRIBE_ACT {
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

        // TODO merge code with deployment.rs#verify_jwt()
        //      - put desired `iss` value as an argument to make sure we verify it

        (account, domain)
    };

    let secret = StaticSecret::random_from_rng(chacha20poly1305::aead::OsRng);

    let identity = DecodedClientId(decode_key(&project.authentication_public_key)?);

    let now = Utc::now();
    let response_message = SubscriptionResponseAuth {
        shared_claims: SharedClaims {
            iat: now.timestamp() as u64,
            exp: add_ttl(now, NOTIFY_SUBSCRIBE_RESPONSE_TTL).timestamp() as u64,
            iss: identity.to_did_key(),
            aud: sub_auth.shared_claims.iss.clone(),
            act: NOTIFY_SUBSCRIBE_RESPONSE_ACT.to_owned(),
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

    // Technically we don't need to derive based on client_public_key anymore; we just need a symkey. But this is technical
    // debt from when clients derived the same symkey on their end via Diffie-Hellman. But now they use the value from
    // watch subscriptions.
    let notify_key = derive_key(&client_public_key, &secret)?;

    let envelope = Envelope::<EnvelopeType0>::new(&sym_key, response)?;

    let base64_notification = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    let scope = parse_scope(&sub_auth.scp)?;

    let notify_topic = topic_from_key(&notify_key);

    let project_id = project.project_id;
    info!(
        "Registering account: {account} with topic: {notify_topic} at project: {project_id}. \
         Scope: {scope:?}. RPC ID: {id:?}",
    );

    info!("Timing: Upserting subscriber");
    let subscriber_id = upsert_subscriber(
        project.id,
        account.clone(),
        scope.clone(),
        &notify_key,
        notify_topic.clone(),
        &state.postgres,
        state.metrics.as_ref(),
    )
    .await?;
    info!("Timing: Finished upserting subscriber");

    // TODO do in same transaction as upsert_subscriber()
    state
        .notify_webhook(
            project_id.as_ref(),
            WebhookNotificationEvent::Subscribed,
            account.as_ref(),
        )
        .await?;

    info!("Timing: Subscribing to notify_topic: {notify_topic}");
    state
        .relay_ws_client
        .subscribe(notify_topic.clone())
        .await?;
    info!("Timing: Finished subscribing to topic");

    info!("Timing: Recording SubscriberUpdateParams");
    state.analytics.client(SubscriberUpdateParams {
        project_pk: project.id,
        project_id,
        pk: subscriber_id,
        account: account.clone(),
        updated_by_iss: sub_auth.shared_claims.iss.into(),
        updated_by_domain: siwe_domain,
        method: NotifyClientMethod::Subscribe,
        old_scope: HashSet::new(),
        new_scope: scope.clone(),
        notification_topic: notify_topic.clone(),
        topic,
    });
    info!("Timing: Finished recording SubscriberUpdateParams");

    // Send noop to extend ttl of relay's mapping
    info!("Timing: Publishing noop to notify_topic");
    publish_relay_message(
        &state.relay_http_client,
        &Publish {
            topic: notify_topic,
            message: {
                // Extremely minor performance optimization with OnceLock to avoid allocating the same empty string everytime
                static LOCK: OnceLock<Arc<str>> = OnceLock::new();
                LOCK.get_or_init(|| "".into()).clone()
            },
            tag: NOTIFY_NOOP_TAG,
            ttl_secs: NOTIFY_NOOP_TTL.as_secs() as u32,
            prompt: false,
        },
        state.metrics.as_ref(),
    )
    .await?;
    info!("Timing: Finished publishing noop to notify_topic");

    info!("Publishing subscribe response to topic: {response_topic}");
    publish_relay_message(
        &state.relay_http_client,
        &Publish {
            topic: response_topic,
            message: base64_notification.into(),
            tag: NOTIFY_SUBSCRIBE_RESPONSE_TAG,
            ttl_secs: NOTIFY_SUBSCRIBE_RESPONSE_TTL.as_secs() as u32,
            prompt: false,
        },
        state.metrics.as_ref(),
    )
    .await?;
    info!("Finished publishing subscribe response");

    let welcome_notification =
        get_welcome_notification(project.id, &state.postgres, state.metrics.as_ref()).await?;
    if let Some(welcome_notification) = welcome_notification {
        info!("Welcome notification enabled");
        if welcome_notification.enabled && scope.contains(&welcome_notification.r#type) {
            info!("Scope contains welcome notification type, sending welcome notification");
            let notification = upsert_notification(
                Uuid::new_v4().to_string(),
                project.id,
                Notification {
                    r#type: welcome_notification.r#type,
                    title: welcome_notification.title,
                    body: welcome_notification.body,
                    url: welcome_notification.url,
                    icon: None,
                },
                &state.postgres,
                state.metrics.as_ref(),
            )
            .await?;

            upsert_subscriber_notifications(
                notification.id,
                &[subscriber_id],
                &state.postgres,
                state.metrics.as_ref(),
            )
            .await?;
        } else {
            info!("Scope does not contain welcome notification type, not sending welcome notification");
        }
    } else {
        info!("Welcome notification not enabled");
    }

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

pub async fn notify_subscribe_client_rate_limit(
    redis: &Arc<Redis>,
    client_public_key: &PublicKey,
    clock: &Clock,
) -> Result<()> {
    rate_limit::token_bucket(
        redis,
        format!(
            "notify-subscribe-client-{}",
            hex::encode(client_public_key.as_bytes())
        ),
        500,
        chrono::Duration::days(1),
        100,
        clock,
    )
    .await
}

pub async fn notify_subscribe_project_rate_limit(
    redis: &Arc<Redis>,
    topic: &Topic,
    clock: &Clock,
) -> Result<()> {
    rate_limit::token_bucket(
        redis,
        format!("notify-subscribe-project-{topic}"),
        50000,
        chrono::Duration::seconds(1),
        1,
        clock,
    )
    .await
}
