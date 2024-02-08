use {
    crate::{
        analytics::subscriber_update::{NotifyClientMethod, SubscriberUpdateParams},
        auth::{
            add_ttl, from_jwt, sign_jwt, verify_identity, AuthError, Authorization, AuthorizedApp,
            DidWeb, SharedClaims, SubscriptionRequestAuth, SubscriptionResponseAuth,
        },
        error::NotifyServerError,
        model::helpers::{get_project_by_topic, get_welcome_notification, upsert_subscriber},
        publish_relay_message::{
            extend_subscription_ttl, publish_relay_message, subscribe_relay_topic,
        },
        rate_limit::{self, Clock, RateLimitError},
        registry::storage::redis::Redis,
        rpc::{
            decode_key, derive_key, NotifyRequest, NotifyResponse, NotifySubscribe, ResponseAuth,
        },
        services::{
            public_http_server::handlers::relay_webhook::{
                error::{RelayMessageClientError, RelayMessageError, RelayMessageServerError},
                handlers::{
                    decrypt_message,
                    notify_watch_subscriptions::{
                        prepare_subscription_watchers, send_to_subscription_watchers,
                    },
                },
                RelayIncomingMessage,
            },
            publisher_service::helpers::{upsert_notification, upsert_subscriber_notifications},
        },
        spec::{
            NOTIFY_SUBSCRIBE_ACT, NOTIFY_SUBSCRIBE_RESPONSE_ACT, NOTIFY_SUBSCRIBE_RESPONSE_TAG,
            NOTIFY_SUBSCRIBE_RESPONSE_TTL,
        },
        state::{AppState, WebhookNotificationEvent},
        types::{parse_scope, Envelope, EnvelopeType0, EnvelopeType1, Notification},
        utils::topic_from_key,
    },
    base64::Engine,
    chrono::Utc,
    relay_rpc::{
        domain::{DecodedClientId, Topic},
        rpc::Publish,
    },
    std::{collections::HashSet, sync::Arc},
    tracing::{info, instrument},
    uuid::Uuid,
    x25519_dalek::{PublicKey, StaticSecret},
};

// TODO limit each subscription to 15 notification types
// TODO limit each account to max 500 subscriptions

// TODO test idempotency (create subscriber a second time for the same account)
#[instrument(name = "wc_notifySubscribe", skip_all)]
pub async fn handle(msg: RelayIncomingMessage, state: &AppState) -> Result<(), RelayMessageError> {
    let topic = msg.topic;

    if let Some(redis) = state.redis.as_ref() {
        notify_subscribe_project_rate_limit(redis, &topic, &state.clock).await?;
    }

    let project = get_project_by_topic(topic.clone(), &state.postgres, state.metrics.as_ref())
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => NotifyServerError::NoProjectDataForTopic(topic.clone()),
            e => e.into(),
        })
        .map_err(RelayMessageServerError::NotifyServerError)?; // TODO change to client error?
    info!("project.id: {}", project.id);
    let project_client_id = project
        .get_authentication_client_id()
        .map_err(RelayMessageServerError::NotifyServerError)?; // TODO change to client error?

    let envelope = Envelope::<EnvelopeType1>::from_bytes(
        base64::engine::general_purpose::STANDARD
            .decode(msg.message.to_string())
            .map_err(RelayMessageClientError::DecodeMessage)?,
    )
    .map_err(RelayMessageClientError::EnvelopeParseError)?;

    let client_public_key = x25519_dalek::PublicKey::from(envelope.pubkey());

    if let Some(redis) = state.redis.as_ref() {
        notify_subscribe_client_rate_limit(redis, &client_public_key, &state.clock).await?;
    }

    let sym_key = derive_key(
        &client_public_key,
        &x25519_dalek::StaticSecret::from(
            decode_key(&project.subscribe_private_key)
                .map_err(RelayMessageServerError::NotifyServerError)?, // TODO change to client error?
        ),
    )
    .map_err(RelayMessageServerError::NotifyServerError)?; // TODO change to client error?
    let response_topic = topic_from_key(&sym_key);
    info!("response_topic: {response_topic}");

    let msg: NotifyRequest<NotifySubscribe> =
        decrypt_message(envelope, &sym_key).map_err(RelayMessageServerError::NotifyServerError)?; // TODO change to client error?

    let request_auth = from_jwt::<SubscriptionRequestAuth>(&msg.params.subscription_auth)
        .map_err(RelayMessageClientError::JwtError)?;
    info!(
        "request_auth.shared_claims.iss: {:?}",
        request_auth.shared_claims.iss
    );
    let request_iss_client_id = DecodedClientId::try_from_did_key(&request_auth.shared_claims.iss)
        .map_err(AuthError::JwtIssNotDidKey)
        .map_err(|e| RelayMessageServerError::NotifyServerError(e.into()))?; // TODO change to client error?

    if request_auth.app.domain() != project.app_domain {
        Err(RelayMessageServerError::NotifyServerError(
            NotifyServerError::AppDoesNotMatch,
        ))?; // TODO change to client error?
    }

    let (account, siwe_domain) = {
        if request_auth.shared_claims.act != NOTIFY_SUBSCRIBE_ACT {
            return Err(AuthError::InvalidAct)
                .map_err(|e| RelayMessageServerError::NotifyServerError(e.into()))?;
            // TODO change to client error?
        }

        let Authorization {
            account,
            app,
            domain,
        } = verify_identity(
            &request_iss_client_id,
            &request_auth.ksu,
            &request_auth.sub,
            state.redis.as_ref(),
            &state.provider,
            state.metrics.as_ref(),
        )
        .await
        .map_err(RelayMessageServerError::NotifyServerError)?; // TODO change to client error?

        // TODO verify `sub_auth.aud` matches `project_data.identity_keypair`

        if let AuthorizedApp::Limited(app) = app {
            if app != project.app_domain {
                Err(RelayMessageClientError::AppSubscriptionsUnauthorized)?;
            }
        }

        // TODO merge code with deployment.rs#verify_jwt()
        //      - put desired `iss` value as an argument to make sure we verify it

        (account, domain)
    };

    let scope = parse_scope(&request_auth.scp)
        .map_err(|e| RelayMessageServerError::NotifyServerError(e.into()))?; // TODO change to client error?

    let subscriber = {
        // Technically we don't need to derive based on client_public_key anymore; we just need a symkey. But this is technical
        // debt from when clients derived the same symkey on their end via Diffie-Hellman. But now they use the value from
        // watch subscriptions.
        let secret = StaticSecret::random_from_rng(chacha20poly1305::aead::OsRng);
        let notify_key = derive_key(&client_public_key, &secret)
            .map_err(RelayMessageServerError::NotifyServerError)?; // TODO change to client error?
        let notify_topic = topic_from_key(&notify_key);

        info!("Timing: Upserting subscriber");
        upsert_subscriber(
            project.id,
            account.clone(),
            scope.clone(),
            &notify_key,
            notify_topic,
            &state.postgres,
            state.metrics.as_ref(),
        )
        .await
        .map_err(|e| RelayMessageServerError::NotifyServerError(e.into()))? // TODO change to client error?
    };
    info!("Timing: Finished upserting subscriber");

    let notify_topic = subscriber.topic;

    // TODO do in same transaction as upsert_subscriber()
    state
        .notify_webhook(
            project.project_id.as_ref(),
            // TODO uncomment when `WebhookNotificationEvent::Updated` exists
            // if subscriber.inserted {
            WebhookNotificationEvent::Subscribed,
            // } else {
            // WebhookNotificationEvent::Updated
            // },
            account.as_ref(),
        )
        .await
        .map_err(RelayMessageServerError::NotifyServerError)?; // TODO change to client error?

    info!("Timing: Subscribing to notify_topic: {notify_topic}");
    subscribe_relay_topic(&state.relay_client, &notify_topic, state.metrics.as_ref())
        .await
        .map_err(|e| RelayMessageServerError::NotifyServerError(e.into()))?;
    info!("Timing: Finished subscribing to topic");

    info!("Timing: Recording SubscriberUpdateParams");
    state.analytics.client(SubscriberUpdateParams {
        project_pk: project.id,
        project_id: project.project_id,
        pk: subscriber.id,
        account: subscriber.account, // Use a consistent account for analytics rather than the per-request one
        updated_by_iss: request_iss_client_id.to_did_key().into(),
        updated_by_domain: siwe_domain,
        method: NotifyClientMethod::Subscribe,
        old_scope: HashSet::new(),
        new_scope: scope.clone(),
        notification_topic: notify_topic.clone(),
        topic,
    });
    info!("Timing: Finished recording SubscriberUpdateParams");

    let (sbs, watchers_with_subscriptions) = prepare_subscription_watchers(
        &request_iss_client_id,
        &request_auth.shared_claims.mjv,
        &account,
        &project.app_domain,
        &state.postgres,
        state.metrics.as_ref(),
    )
    .await
    .map_err(RelayMessageServerError::NotifyServerError)?; // TODO change to client error?

    {
        let now = Utc::now();
        let response_message = SubscriptionResponseAuth {
            shared_claims: SharedClaims {
                iat: now.timestamp() as u64,
                exp: add_ttl(now, NOTIFY_SUBSCRIBE_RESPONSE_TTL).timestamp() as u64,
                iss: project_client_id.to_did_key(),
                aud: request_iss_client_id.to_did_key(),
                act: NOTIFY_SUBSCRIBE_RESPONSE_ACT.to_owned(),
                mjv: "1".to_owned(),
            },
            sub: account.to_did_pkh(),
            app: DidWeb::from_domain(project.app_domain.clone()),
            sbs,
        };
        let response_auth = sign_jwt(
            response_message,
            &ed25519_dalek::SigningKey::from_bytes(
                &decode_key(&project.authentication_private_key)
                    .map_err(RelayMessageServerError::NotifyServerError)?, // TODO change to client error?
            ),
        )
        .map_err(RelayMessageServerError::NotifyServerError)?; // TODO change to client error?
        let response = NotifyResponse::new(msg.id, ResponseAuth { response_auth });
        let envelope = Envelope::<EnvelopeType0>::new(&sym_key, response)
            .map_err(RelayMessageServerError::NotifyServerError)?; // TODO change to client error?
        let base64_notification =
            base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

        info!("Publishing subscribe response to topic: {response_topic}");
        publish_relay_message(
            &state.relay_client,
            &Publish {
                topic: response_topic,
                message: base64_notification.into(),
                tag: NOTIFY_SUBSCRIBE_RESPONSE_TAG,
                ttl_secs: NOTIFY_SUBSCRIBE_RESPONSE_TTL.as_secs() as u32,
                prompt: false,
            },
            state.metrics.as_ref(),
        )
        .await
        .map_err(|e| RelayMessageServerError::NotifyServerError(e.into()))?; // TODO change to client error?
        info!("Finished publishing subscribe response");
    }

    extend_subscription_ttl(&state.relay_client, notify_topic, state.metrics.as_ref())
        .await
        .map_err(|e| RelayMessageServerError::NotifyServerError(e.into()))?; // TODO change to client error?

    // TODO do in same txn as upsert_subscriber()
    if subscriber.inserted {
        let welcome_notification =
            get_welcome_notification(project.id, &state.postgres, state.metrics.as_ref())
                .await
                .map_err(|e| RelayMessageServerError::NotifyServerError(e.into()))?; // TODO change to client error?
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
                .await
                .map_err(|e| RelayMessageServerError::NotifyServerError(e.into()))?; // TODO change to client error?

                upsert_subscriber_notifications(
                    notification.id,
                    &[subscriber.id],
                    &state.postgres,
                    state.metrics.as_ref(),
                )
                .await
                .map_err(|e| RelayMessageServerError::NotifyServerError(e.into()))?;
            // TODO change to client error?
            } else {
                info!("Scope does not contain welcome notification type, not sending welcome notification");
            }
        } else {
            info!("Welcome notification not enabled");
        }
    } else {
        info!("Subscriber already existed, not sending welcome notification");
    }

    send_to_subscription_watchers(
        watchers_with_subscriptions,
        &state.notify_keys.authentication_secret,
        &state.notify_keys.authentication_client_id,
        &state.relay_client,
        state.metrics.as_ref(),
    )
    .await
    .map_err(RelayMessageServerError::NotifyServerError)?; // TODO change to client error?

    Ok(())
}

pub async fn notify_subscribe_client_rate_limit(
    redis: &Arc<Redis>,
    client_public_key: &PublicKey,
    clock: &Clock,
) -> Result<(), RateLimitError> {
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
) -> Result<(), RateLimitError> {
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
