use {
    crate::{
        analytics::subscriber_update::{NotifyClientMethod, SubscriberUpdateParams},
        auth::{
            add_ttl, from_jwt, sign_jwt, verify_identity, AuthError, Authorization, AuthorizedApp,
            DidWeb, NotifyServerSubscription, SharedClaims, SubscriptionDeleteRequestAuth,
            SubscriptionDeleteResponseAuth,
        },
        error::NotifyServerError,
        model::{
            helpers::{
                delete_subscriber, get_project_by_id, get_subscriber_by_topic, SubscriberWithScope,
                SubscriptionWatcherQuery,
            },
            types::Project,
        },
        publish_relay_message::publish_relay_message,
        rate_limit::{self, Clock, RateLimitError},
        registry::storage::redis::Redis,
        rpc::{
            decode_key, JsonRpcRequest, JsonRpcResponse, JsonRpcResponseError, NotifyDelete,
            ResponseAuth,
        },
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
        domain::{DecodedClientId, SubscriptionId, Topic},
        rpc::Publish,
    },
    std::{collections::HashSet, sync::Arc},
    tracing::{info, warn},
};

// TODO make and test idempotency
pub async fn handle(msg: RelayIncomingMessage, state: &AppState) -> Result<(), RelayMessageError> {
    if let Some(redis) = state.redis.as_ref() {
        notify_delete_rate_limit(redis, &msg.topic, &state.clock).await?;
    }

    // TODO combine these two SQL queries
    let subscriber =
        get_subscriber_by_topic(msg.topic.clone(), &state.postgres, state.metrics.as_ref())
            .await
            .map_err(|e| match e {
                sqlx::Error::RowNotFound => RelayMessageError::Client(
                    RelayMessageClientError::WrongNotifyDeleteTopic(msg.topic.clone()),
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
    if msg.topic != topic_from_key(&sym_key) {
        return Err(RelayMessageServerError::NotifyServerError(
            NotifyServerError::TopicDoesNotMatchKey,
        ))?; // TODO change to client error?
    }

    let req = decrypt_message::<NotifyDelete, _>(envelope, &sym_key)
        .map_err(RelayMessageServerError::NotifyServerError)?; // TODO change to client error?

    async fn handle(
        state: &AppState,
        msg: &RelayIncomingMessage,
        req: &JsonRpcRequest<NotifyDelete>,
        subscriber: &SubscriberWithScope,
        project: &Project,
        project_client_id: DecodedClientId,
    ) -> Result<
        (
            ResponseAuth,
            Vec<(SubscriptionWatcherQuery, Vec<NotifyServerSubscription>)>,
        ),
        RelayMessageError,
    > {
        info!("req.id: {}", req.id);
        info!("req.jsonrpc: {}", req.jsonrpc); // TODO verify this
        info!("req.method: {}", req.method); // TODO verify this

        let request_auth = from_jwt::<SubscriptionDeleteRequestAuth>(&req.params.delete_auth)
            .map_err(RelayMessageClientError::JwtError)?;
        info!(
            "request_auth.shared_claims.iss: {:?}",
            request_auth.shared_claims.iss
        );
        let request_iss_client_id =
            DecodedClientId::try_from_did_key(&request_auth.shared_claims.iss)
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
            let topic = msg.topic.clone();
            async move {
                // Relay ignores subscription_id, generate a random one since we don't have it here.
                // https://walletconnect.slack.com/archives/C05ABTQSPFY/p1706337410799659?thread_ts=1706307603.828609&cid=C05ABTQSPFY
                let subscription_id = SubscriptionId::generate();
                if let Err(e) = relay_client.unsubscribe(topic, subscription_id).await {
                    warn!("Error unsubscribing Notify from topic: {}", e);
                }
            }
        });

        state.analytics.client(SubscriberUpdateParams {
            project_pk: project.id,
            project_id: project.project_id.clone(),
            pk: subscriber.id,
            account: subscriber.account.clone(), // Use a consistent account for analytics rather than the per-request one
            updated_by_iss: request_auth.shared_claims.iss.clone().into(),
            updated_by_domain: siwe_domain,
            method: NotifyClientMethod::Unsubscribe,
            old_scope: subscriber.scope.iter().cloned().map(Into::into).collect(),
            new_scope: HashSet::new(),
            notification_topic: subscriber.topic.clone(),
            topic: msg.topic.clone(),
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

        Ok((ResponseAuth { response_auth }, watchers_with_subscriptions))
    }

    let result = handle(state, &msg, &req, &subscriber, &project, project_client_id).await;

    let (response, watchers_with_subscriptions) = match result {
        Ok((result, watchers_with_subscriptions)) => (
            serde_json::to_vec(&JsonRpcResponse::new(req.id, result))
                .map_err(Into::into)
                .map_err(RelayMessageServerError::NotifyServerError)?,
            Some(watchers_with_subscriptions),
        ),
        Err(e) => (
            serde_json::to_vec(&JsonRpcResponseError::new(req.id, e.to_string()))
                .map_err(Into::into)
                .map_err(RelayMessageServerError::NotifyServerError)?,
            None,
        ),
    };

    let response_fut = async {
        let envelope = Envelope::<EnvelopeType0>::new(&sym_key, response)
            .map_err(RelayMessageServerError::NotifyServerError)?; // TODO change to client error?
        let base64_notification =
            base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

        publish_relay_message(
            &state.relay_client,
            &Publish {
                topic: msg.topic,
                message: base64_notification.into(),
                tag: NOTIFY_DELETE_RESPONSE_TAG,
                ttl_secs: NOTIFY_DELETE_RESPONSE_TTL.as_secs() as u32,
                prompt: false,
            },
            state.metrics.as_ref(),
        )
        .await
        .map_err(Into::into)
        .map_err(RelayMessageServerError::NotifyServerError) // TODO change to client error?
    };

    if let Some(watchers_with_subscriptions) = watchers_with_subscriptions {
        let watcher_fut = async {
            send_to_subscription_watchers(
                watchers_with_subscriptions,
                &state.notify_keys.authentication_secret,
                &state.notify_keys.authentication_client_id,
                &state.relay_client,
                state.metrics.as_ref(),
            )
            .await
            .map_err(Into::into)
        };

        tokio::try_join!(response_fut, watcher_fut)?;
    } else {
        response_fut.await?;
    }

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
