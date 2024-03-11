use {
    crate::{
        analytics::subscriber_update::{NotifyClientMethod, SubscriberUpdateParams},
        auth::{
            add_ttl, from_jwt, sign_jwt, verify_identity, AuthError, Authorization, AuthorizedApp,
            DidWeb, SharedClaims, SubscriptionDeleteRequestAuth, SubscriptionDeleteResponseAuth,
        },
        error::NotifyServerError,
        model::helpers::{delete_subscriber, get_project_by_id, get_subscriber_by_topic},
        publish_relay_message::publish_relay_message,
        rate_limit::{self, Clock, RateLimitError},
        registry::storage::redis::Redis,
        rpc::{decode_key, JsonRpcResponse, NotifyDelete, ResponseAuth},
        services::public_http_server::handlers::relay_webhook::{
            error::{RelayMessageClientError, RelayMessageError, RelayMessageServerError},
            handlers::{
                decrypt_message,
                notify_watch_subscriptions::{
                    prepare_subscription_watchers, send_to_subscription_watchers,
                },
            },
            RelayIncomingMessage,
        },
        spec::{
            NOTIFY_DELETE_ACT, NOTIFY_DELETE_RESPONSE_ACT, NOTIFY_DELETE_RESPONSE_TAG,
            NOTIFY_DELETE_RESPONSE_TTL,
        },
        state::{AppState, WebhookNotificationEvent},
        types::{Envelope, EnvelopeType0},
        utils::{is_same_address, topic_from_key},
    },
    base64::Engine,
    chrono::Utc,
    relay_rpc::{
        auth::ed25519_dalek::SigningKey,
        domain::{DecodedClientId, Topic},
        rpc::Publish,
    },
    std::{collections::HashSet, sync::Arc},
    tracing::{info, warn},
};

// TODO make and test idempotency
pub async fn handle(msg: RelayIncomingMessage, state: &AppState) -> Result<(), RelayMessageError> {
    let topic = msg.topic;

    if let Some(redis) = state.redis.as_ref() {
        notify_delete_rate_limit(redis, &topic, &state.clock).await?;
    }

    // TODO combine these two SQL queries
    let subscriber =
        get_subscriber_by_topic(topic.clone(), &state.postgres, state.metrics.as_ref())
            .await
            .map_err(|e| match e {
                sqlx::Error::RowNotFound => RelayMessageError::Client(
                    RelayMessageClientError::WrongNotifyDeleteTopic(topic.clone()),
                ),
                e => {
                    RelayMessageError::Server(RelayMessageServerError::NotifyServerError(e.into()))
                }
            })?;
    let project = get_project_by_id(subscriber.project, &state.postgres, state.metrics.as_ref())
        .await
        .map_err(|e| RelayMessageServerError::NotifyServerError(e.into()))?; // TODO change to client error?
    info!("project.id: {}", project.id);
    let project_client_id = project
        .get_authentication_client_id()
        .map_err(RelayMessageServerError::NotifyServerError)?; // TODO change to client error?

    let envelope = Envelope::<EnvelopeType0>::from_bytes(
        base64::engine::general_purpose::STANDARD
            .decode(msg.message.to_string())
            .map_err(RelayMessageClientError::DecodeMessage)?,
    )
    .map_err(RelayMessageClientError::EnvelopeParseError)?;

    let sym_key =
        decode_key(&subscriber.sym_key).map_err(RelayMessageServerError::NotifyServerError)?; // TODO change to client error?

    let msg = decrypt_message::<NotifyDelete, _>(envelope, &sym_key)
        .map_err(RelayMessageServerError::NotifyServerError)?; // TODO change to client error?
    info!("msg.id: {}", msg.id);
    info!("msg.jsonrpc: {}", msg.jsonrpc); // TODO verify this
    info!("msg.method: {}", msg.method); // TODO verify this

    let request_auth = from_jwt::<SubscriptionDeleteRequestAuth>(&msg.params.delete_auth)
        .map_err(RelayMessageClientError::JwtError)?;
    info!(
        "request_auth.shared_claims.iss: {:?}",
        request_auth.shared_claims.iss
    );
    let request_iss_client_id = DecodedClientId::try_from_did_key(&request_auth.shared_claims.iss)
        .map_err(AuthError::JwtIssNotDidKey)
        .map_err(|e| RelayMessageServerError::NotifyServerError(e.into()))?; // TODO change to client error?

    if request_auth.app.domain() != project.app_domain {
        Err(RelayMessageClientError::AppDoesNotMatch)?;
    }

    let (account, siwe_domain) = {
        if request_auth.shared_claims.act != NOTIFY_DELETE_ACT {
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
        .await?;

        // TODO verify `sub_auth.aud` matches `project_data.identity_keypair`

        if let AuthorizedApp::Limited(app) = app {
            if app != project.app_domain {
                Err(RelayMessageClientError::AppSubscriptionsUnauthorized)?;
            }
        }

        if !is_same_address(&account, &subscriber.account) {
            Err(RelayMessageServerError::NotifyServerError(
                NotifyServerError::AccountNotAuthorized,
            ))?; // TODO change to client error?
        }

        (account, domain)
    };

    delete_subscriber(subscriber.id, &state.postgres, state.metrics.as_ref())
        .await
        .map_err(|e| RelayMessageServerError::NotifyServerError(e.into()))?; // TODO change to client error?

    // TODO do in same txn as delete_subscriber()
    state
        .notify_webhook(
            project.project_id.as_ref(),
            WebhookNotificationEvent::Unsubscribed,
            subscriber.account.as_ref(),
        )
        .await
        .map_err(RelayMessageServerError::NotifyServerError)?; // TODO change to client error?

    tokio::task::spawn({
        let relay_client = state.relay_client.clone();
        let topic = topic.clone();
        async move {
            if let Err(e) = relay_client.unsubscribe(topic).await {
                warn!("Error unsubscribing Notify from topic: {}", e);
            }
        }
    });

    state.analytics.client(SubscriberUpdateParams {
        project_pk: project.id,
        project_id: project.project_id,
        pk: subscriber.id,
        account: subscriber.account, // Use a consistent account for analytics rather than the per-request one
        updated_by_iss: request_auth.shared_claims.iss.clone().into(),
        updated_by_domain: siwe_domain,
        method: NotifyClientMethod::Unsubscribe,
        old_scope: subscriber.scope.into_iter().map(Into::into).collect(),
        new_scope: HashSet::new(),
        notification_topic: subscriber.topic,
        topic,
    });

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

    let response_fut = async {
        let now = Utc::now();
        let response_message = SubscriptionDeleteResponseAuth {
            shared_claims: SharedClaims {
                iat: now.timestamp() as u64,
                exp: add_ttl(now, NOTIFY_DELETE_RESPONSE_TTL).timestamp() as u64,
                iss: project_client_id.to_did_key(),
                aud: request_auth.shared_claims.iss,
                act: NOTIFY_DELETE_RESPONSE_ACT.to_owned(),
                mjv: "1".to_owned(),
            },
            sub: account.to_did_pkh(),
            app: DidWeb::from_domain(project.app_domain.clone()),
            sbs,
        };
        let response_auth = sign_jwt(
            response_message,
            &SigningKey::from_bytes(
                &decode_key(&project.authentication_private_key)
                    .map_err(RelayMessageServerError::NotifyServerError)?, // TODO change to client error?
            ),
        )
        .map_err(RelayMessageServerError::NotifyServerError)?; // TODO change to client error?
        let response = JsonRpcResponse::new(msg.id, ResponseAuth { response_auth });
        let envelope = Envelope::<EnvelopeType0>::new(&sym_key, response)
            .map_err(RelayMessageServerError::NotifyServerError)?; // TODO change to client error?
        let base64_notification =
            base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

        publish_relay_message(
            &state.relay_client,
            &Publish {
                topic: topic_from_key(&sym_key),
                message: base64_notification.into(),
                tag: NOTIFY_DELETE_RESPONSE_TAG,
                ttl_secs: NOTIFY_DELETE_RESPONSE_TTL.as_secs() as u32,
                prompt: false,
            },
            state.metrics.as_ref(),
        )
        .await
        .map_err(|e| RelayMessageServerError::NotifyServerError(e.into())) // TODO change to client error?
    };

    let watcher_fut = async {
        send_to_subscription_watchers(
            watchers_with_subscriptions,
            &state.notify_keys.authentication_secret,
            &state.notify_keys.authentication_client_id,
            &state.relay_client,
            state.metrics.as_ref(),
        )
        .await
        .map_err(RelayMessageServerError::NotifyServerError) // TODO change to client error?
    };

    tokio::try_join!(response_fut, watcher_fut)?;
    Ok(())
}

pub async fn notify_delete_rate_limit(
    redis: &Arc<Redis>,
    topic: &Topic,
    clock: &Clock,
) -> Result<(), RateLimitError> {
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
