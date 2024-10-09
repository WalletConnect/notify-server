use {
    super::notify_watch_subscriptions::prepare_subscription_watchers,
    crate::{
        analytics::subscriber_update::{NotifyClientMethod, SubscriberUpdateParams},
        auth::{
            add_ttl, from_jwt, sign_jwt, verify_identity, AuthError, Authorization, AuthorizedApp,
            DidWeb, NotifyServerSubscription, SharedClaims, SubscriptionUpdateRequestAuth,
            SubscriptionUpdateResponseAuth,
        },
        model::{
            helpers::{
                get_project_by_id, get_subscriber_by_topic, update_subscriber, SubscriberWithScope,
                SubscriptionWatcherQuery,
            },
            types::Project,
        },
        publish_relay_message::publish_relay_message,
        rate_limit::{self, Clock, RateLimitError},
        registry::storage::redis::Redis,
        rpc::{
            decode_key, JsonRpcRequest, JsonRpcResponse, JsonRpcResponseError, NotifyUpdate,
            ResponseAuth,
        },
        services::public_http_server::handlers::relay_webhook::{
            error::{RelayMessageClientError, RelayMessageError, RelayMessageServerError},
            handlers::{
                decrypt_message, notify_watch_subscriptions::send_to_subscription_watchers,
            },
            RelayIncomingMessage,
        },
        spec::{
            NOTIFY_UPDATE_ACT, NOTIFY_UPDATE_RESPONSE_ACT, NOTIFY_UPDATE_RESPONSE_TAG,
            NOTIFY_UPDATE_RESPONSE_TTL,
        },
        state::AppState,
        types::{parse_scope, Envelope, EnvelopeType0},
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
    tracing::info,
};

// TODO test idempotency
pub async fn handle(msg: RelayIncomingMessage, state: &AppState) -> Result<(), RelayMessageError> {
    if let Some(redis) = state.redis.as_ref() {
        notify_update_rate_limit(redis, &msg.topic, &state.clock).await?;
    }

    // TODO combine these two SQL queries
    let subscriber =
        get_subscriber_by_topic(msg.topic.clone(), &state.postgres, state.metrics.as_ref())
            .await
            .map_err(|e| match e {
                sqlx::Error::RowNotFound => RelayMessageError::Client(
                    RelayMessageClientError::WrongNotifyUpdateTopic(msg.topic.clone()),
                ),
                e => RelayMessageError::Server(RelayMessageServerError::NotifyServer(e.into())),
            })?;
    let project = get_project_by_id(subscriber.project, &state.postgres, state.metrics.as_ref())
        .await
        .map_err(|e| RelayMessageServerError::NotifyServer(e.into()))?; // TODO change to client error?
    info!("project.id: {}", project.id);
    let project_client_id = project
        .get_authentication_client_id()
        .map_err(RelayMessageServerError::GetAuthenticationClientId)?;

    let envelope = Envelope::<EnvelopeType0>::from_bytes(
        base64::engine::general_purpose::STANDARD
            .decode(msg.message.to_string())
            .map_err(RelayMessageClientError::DecodeMessage)?,
    )
    .map_err(RelayMessageClientError::EnvelopeParse)?;

    let sym_key = decode_key(&subscriber.sym_key).map_err(RelayMessageServerError::DecodeKey)?;
    if msg.topic != topic_from_key(&sym_key) {
        Err(RelayMessageClientError::TopicDoesNotMatchKey)?;
    }

    let req = decrypt_message::<NotifyUpdate, _>(envelope, &sym_key)?;

    async fn handle(
        state: &AppState,
        msg: &RelayIncomingMessage,
        req: &JsonRpcRequest<NotifyUpdate>,
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

        let request_auth = from_jwt::<SubscriptionUpdateRequestAuth>(&req.params.update_auth)
            .map_err(RelayMessageClientError::Jwt)?;
        info!(
            "request_auth.shared_claims.iss: {:?}",
            request_auth.shared_claims.iss
        );
        let request_iss_client_id =
            DecodedClientId::try_from_did_key(&request_auth.shared_claims.iss)
                .map_err(AuthError::JwtIssNotDidKey)
                .map_err(|e| RelayMessageServerError::NotifyServer(e.into()))?; // TODO change to client error?
        if request_auth.app.domain() != project.app_domain {
            Err(RelayMessageClientError::AppDoesNotMatch)?;
        }

        let (account, siwe_domain) = {
            if request_auth.shared_claims.act != NOTIFY_UPDATE_ACT {
                return Err(AuthError::InvalidAct)
                    .map_err(|e| RelayMessageServerError::NotifyServer(e.into()))?;
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
                state.provider.as_ref(),
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
                Err(RelayMessageClientError::AccountNotAuthorized)?;
            }

            (account, domain)
        };

        let old_scope = subscriber.scope.iter().cloned().collect::<HashSet<_>>();
        let new_scope = parse_scope(&request_auth.scp)
            .map_err(|e| RelayMessageServerError::NotifyServer(e.into()))?; // TODO change to client error?

        let subscriber = update_subscriber(
            subscriber.id,
            new_scope.clone(),
            &state.postgres,
            state.metrics.as_ref(),
        )
        .await
        .map_err(|e| RelayMessageServerError::NotifyServer(e.into()))?; // TODO change to client error?

        // TODO do in same transaction as update_subscriber()
        // state
        //     .notify_webhook(
        //         project_id.as_ref(),
        //         WebhookNotificationEvent::Updated,
        //         account.as_ref(),
        //     )
        //     .await?;

        state.analytics.subscriber_update(SubscriberUpdateParams {
            project_pk: project.id,
            project_id: project.project_id.clone(),
            pk: subscriber.id,
            account: subscriber.account, // Use a consistent account for analytics rather than the per-request one
            updated_by_iss: request_auth.shared_claims.iss.clone().into(),
            updated_by_domain: siwe_domain,
            method: NotifyClientMethod::Update,
            old_scope,
            new_scope,
            notification_topic: subscriber.topic.clone(),
            topic: msg.topic.clone(),
        });

        // Problem: this doesn't send an update to the watcher that made the change (by design) but
        // the request was to send these updates to all watchers for mark_notifications_as_read
        let (sbs, watchers_with_subscriptions) = prepare_subscription_watchers(
            &request_iss_client_id,
            &request_auth.shared_claims.mjv,
            &account,
            &project.app_domain,
            &state.postgres,
            state.metrics.as_ref(),
        )
        .await
        .map_err(RelayMessageServerError::PrepareSubscriptionWatchers)?;

        let now = Utc::now();
        let response_auth = SubscriptionUpdateResponseAuth {
            shared_claims: SharedClaims {
                iat: now.timestamp() as u64,
                exp: add_ttl(now, NOTIFY_UPDATE_RESPONSE_TTL).timestamp() as u64,
                iss: project_client_id.to_did_key(),
                aud: request_iss_client_id.to_did_key(),
                act: NOTIFY_UPDATE_RESPONSE_ACT.to_owned(),
                mjv: "1".to_owned(),
            },
            sub: account.to_did_pkh(),
            app: DidWeb::from_domain(project.app_domain.clone()),
            sbs,
        };
        let response_auth = sign_jwt(
            response_auth,
            &SigningKey::from_bytes(
                &decode_key(&project.authentication_private_key)
                    .map_err(RelayMessageServerError::DecodeKey)?,
            ),
        )
        .map_err(RelayMessageServerError::SignJwt)?;

        Ok((ResponseAuth { response_auth }, watchers_with_subscriptions))
    }

    let result = handle(state, &msg, &req, &subscriber, &project, project_client_id).await;

    let (response, watchers_with_subscriptions, result) = match result {
        Ok((result, watchers_with_subscriptions)) => (
            serde_json::to_vec(&JsonRpcResponse::new(req.id, result))
                .map_err(RelayMessageServerError::JsonRpcResponseSerialization)?,
            Some(watchers_with_subscriptions),
            Ok(()),
        ),
        Err(e) => (
            serde_json::to_vec(&JsonRpcResponseError::new(req.id, (&e).into()))
                .map_err(RelayMessageServerError::JsonRpcResponseErrorSerialization)?,
            None,
            Err(e),
        ),
    };

    let msg = Arc::new(msg);

    let response_fut = {
        let msg = msg.clone();
        async {
            let envelope = Envelope::<EnvelopeType0>::new(&sym_key, response)
                .map_err(RelayMessageServerError::EnvelopeEncryption)?;
            let base64_notification =
                base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

            publish_relay_message(
                &state.relay_client,
                &Publish {
                    topic: msg.topic.clone(),
                    message: base64_notification.into(),
                    tag: NOTIFY_UPDATE_RESPONSE_TAG,
                    ttl_secs: NOTIFY_UPDATE_RESPONSE_TTL.as_secs() as u32,
                    prompt: false,
                },
                Some(msg),
                state.metrics.as_ref(),
                &state.analytics,
            )
            .await
            .map_err(Into::into)
            .map_err(RelayMessageServerError::NotifyServer)
        }
    };

    if let Some(watchers_with_subscriptions) = watchers_with_subscriptions {
        let watcher_fut = async {
            send_to_subscription_watchers(
                watchers_with_subscriptions,
                &state.notify_keys.authentication_secret,
                &state.notify_keys.authentication_client_id,
                &state.relay_client,
                msg,
                state.metrics.as_ref(),
                &state.analytics,
            )
            .await
            .map_err(RelayMessageServerError::SubscriptionWatcherSend)
        };

        tokio::try_join!(response_fut, watcher_fut)?;
    } else {
        response_fut.await?;
    }

    result
}

pub async fn notify_update_rate_limit(
    redis: &Arc<Redis>,
    topic: &Topic,
    clock: &Clock,
) -> Result<(), RateLimitError> {
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
