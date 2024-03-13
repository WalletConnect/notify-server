use {
    crate::{
        analytics::get_notifications::GetNotificationsParams,
        auth::{
            add_ttl, from_jwt, sign_jwt, verify_identity, AuthError, Authorization, AuthorizedApp,
            DidWeb, SharedClaims, SubscriptionGetNotificationsRequestAuth,
            SubscriptionGetNotificationsResponseAuth,
        },
        error::NotifyServerError,
        model::{
            helpers::{
                get_notifications_for_subscriber, get_project_by_id, get_subscriber_by_topic,
                SubscriberWithScope,
            },
            types::Project,
        },
        publish_relay_message::publish_relay_message,
        rate_limit::{self, Clock, RateLimitError},
        registry::storage::redis::Redis,
        rpc::{decode_key, AuthMessage, JsonRpcRequest, JsonRpcResponse, JsonRpcResponseError},
        services::public_http_server::handlers::relay_webhook::{
            error::{RelayMessageClientError, RelayMessageError, RelayMessageServerError},
            handlers::decrypt_message,
            RelayIncomingMessage,
        },
        spec::{
            NOTIFY_GET_NOTIFICATIONS_ACT, NOTIFY_GET_NOTIFICATIONS_RESPONSE_ACT,
            NOTIFY_GET_NOTIFICATIONS_RESPONSE_TAG, NOTIFY_GET_NOTIFICATIONS_RESPONSE_TTL,
        },
        state::AppState,
        types::{Envelope, EnvelopeType0},
        utils::topic_from_key,
    },
    base64::Engine,
    chrono::Utc,
    relay_rpc::{
        auth::ed25519_dalek::SigningKey,
        domain::{DecodedClientId, Topic},
        rpc::{msg_id::get_message_id, Publish},
    },
    std::sync::Arc,
    tracing::info,
};

// TODO test idempotency
pub async fn handle(msg: RelayIncomingMessage, state: &AppState) -> Result<(), RelayMessageError> {
    if let Some(redis) = state.redis.as_ref() {
        notify_get_notifications_rate_limit(redis, &msg.topic, &state.clock).await?;
    }

    // TODO combine these two SQL queries
    let subscriber =
        get_subscriber_by_topic(msg.topic.clone(), &state.postgres, state.metrics.as_ref())
            .await
            .map_err(|e| match e {
                sqlx::Error::RowNotFound => RelayMessageError::Client(
                    RelayMessageClientError::WrongNotifyGetNotificationsTopic(msg.topic.clone()),
                ),
                e => {
                    RelayMessageError::Server(RelayMessageServerError::NotifyServerError(e.into()))
                }
            })?;
    let project = get_project_by_id(subscriber.project, &state.postgres, state.metrics.as_ref())
        .await
        .map_err(|e| RelayMessageServerError::NotifyServerError(e.into()))?; // TODO change to client error?
    info!("project.id: {}", project.id);

    let envelope = Envelope::<EnvelopeType0>::from_bytes(
        base64::engine::general_purpose::STANDARD
            .decode(msg.message.to_string())
            .map_err(RelayMessageClientError::DecodeMessage)?,
    )
    .map_err(RelayMessageClientError::EnvelopeParseError)?;

    let sym_key = decode_key(&subscriber.sym_key).map_err(RelayMessageServerError::DecodeKey)?;
    if msg.topic != topic_from_key(&sym_key) {
        return Err(RelayMessageServerError::NotifyServerError(
            NotifyServerError::TopicDoesNotMatchKey,
        ))?; // TODO change to client error?
    }

    let req = decrypt_message::<AuthMessage, _>(envelope, &sym_key)?;

    async fn handle(
        state: &AppState,
        msg: &RelayIncomingMessage,
        req: &JsonRpcRequest<AuthMessage>,
        subscriber: &SubscriberWithScope,
        project: &Project,
    ) -> Result<AuthMessage, RelayMessageError> {
        info!("req.id: {}", req.id);
        info!("req.jsonrpc: {}", req.jsonrpc); // TODO verify this
        info!("req.method: {}", req.method); // TODO verify this

        let request_auth = from_jwt::<SubscriptionGetNotificationsRequestAuth>(&req.params.auth)
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
            if request_auth.shared_claims.act != NOTIFY_GET_NOTIFICATIONS_ACT {
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

            (account, Arc::<str>::from(domain))
        };

        request_auth
            .validate()
            .map_err(RelayMessageServerError::NotifyServerError)?; // TODO change to client error?

        let data = get_notifications_for_subscriber(
            subscriber.id,
            request_auth.params,
            &state.postgres,
            state.metrics.as_ref(),
        )
        .await
        .map_err(|e| RelayMessageServerError::NotifyServerError(e.into()))?; // TODO change to client error?

        let relay_message_id: Arc<str> = get_message_id(msg.message.as_ref()).into();
        for notification in data.notifications.iter() {
            state.analytics.get_notifications(GetNotificationsParams {
                topic: msg.topic.clone(),
                message_id: relay_message_id.clone(),
                get_by_iss: request_auth.shared_claims.iss.clone().into(),
                get_by_domain: siwe_domain.clone(),
                project_pk: project.id,
                project_id: project.project_id.clone(),
                subscriber_pk: subscriber.id,
                subscriber_account: subscriber.account.clone(),
                notification_topic: subscriber.topic.clone(),
                subscriber_notification_id: notification.id,
                notification_id: notification.notification_id,
                notification_type: notification.r#type,
                returned_count: data.notifications.len(),
            });
        }

        let identity = DecodedClientId(
            decode_key(&project.authentication_public_key)
                .map_err(RelayMessageServerError::DecodeKey)?,
        );

        let now = Utc::now();
        let response_message = SubscriptionGetNotificationsResponseAuth {
            shared_claims: SharedClaims {
                iat: now.timestamp() as u64,
                exp: add_ttl(now, NOTIFY_GET_NOTIFICATIONS_RESPONSE_TTL).timestamp() as u64,
                iss: identity.to_did_key(),
                aud: request_iss_client_id.to_did_key(),
                act: NOTIFY_GET_NOTIFICATIONS_RESPONSE_ACT.to_owned(),
                mjv: "1".to_owned(),
            },
            sub: account.to_did_pkh(),
            app: DidWeb::from_domain(project.app_domain.clone()),
            result: data,
        };
        let auth = sign_jwt(
            response_message,
            &SigningKey::from_bytes(
                &decode_key(&project.authentication_private_key)
                    .map_err(RelayMessageServerError::DecodeKey)?,
            ),
        )
        .map_err(RelayMessageServerError::SignJwt)?;
        Ok(AuthMessage { auth })
    }

    let result = handle(state, &msg, &req, &subscriber, &project).await;

    let response = match result {
        Ok(result) => serde_json::to_vec(&JsonRpcResponse::new(req.id, result))
            .map_err(RelayMessageServerError::JsonRpcResponseSerialization)?,
        Err(e) => serde_json::to_vec(&JsonRpcResponseError::new(req.id, e.into()))
            .map_err(RelayMessageServerError::JsonRpcResponseErrorSerialization)?,
    };

    let envelope = Envelope::<EnvelopeType0>::new(&sym_key, response)
        .map_err(RelayMessageServerError::EnvelopeEncryption)?;

    let response = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    publish_relay_message(
        &state.relay_client,
        &Publish {
            topic: msg.topic,
            message: response.into(),
            tag: NOTIFY_GET_NOTIFICATIONS_RESPONSE_TAG,
            ttl_secs: NOTIFY_GET_NOTIFICATIONS_RESPONSE_TTL.as_secs() as u32,
            prompt: false,
        },
        state.metrics.as_ref(),
    )
    .await
    .map_err(|e| RelayMessageServerError::NotifyServerError(e.into()))?; // TODO change to client error?

    Ok(())
}

pub async fn notify_get_notifications_rate_limit(
    redis: &Arc<Redis>,
    topic: &Topic,
    clock: &Clock,
) -> Result<(), RateLimitError> {
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
