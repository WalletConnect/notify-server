use {
    crate::{
        analytics::subscriber_update::{NotifyClientMethod, SubscriberUpdateParams},
        auth::{
            add_ttl, from_jwt, sign_jwt, verify_identity, AuthError, Authorization, AuthorizedApp,
            SharedClaims, SubscriptionDeleteRequestAuth, SubscriptionDeleteResponseAuth,
        },
        error::Error,
        model::helpers::{delete_subscriber, get_project_by_id, get_subscriber_by_topic},
        services::websocket_service::{
            decode_key,
            handlers::{decrypt_message, notify_watch_subscriptions::update_subscription_watchers},
            NotifyDelete, NotifyRequest, NotifyResponse,
        },
        spec::{NOTIFY_DELETE_RESPONSE_TAG, NOTIFY_DELETE_RESPONSE_TTL},
        state::{AppState, WebhookNotificationEvent},
        types::{Envelope, EnvelopeType0},
        Result,
    },
    anyhow::anyhow,
    base64::Engine,
    chrono::Utc,
    relay_client::websocket::{Client, PublishedMessage},
    relay_rpc::domain::DecodedClientId,
    serde_json::{json, Value},
    std::collections::HashSet,
    tracing::warn,
};

// TODO make and test idempotency
pub async fn handle(msg: PublishedMessage, state: &AppState, client: &Client) -> Result<()> {
    let topic = msg.topic;
    let subscription_id = msg.subscription_id;

    // TODO combine these two SQL queries
    let subscriber = get_subscriber_by_topic(topic.clone(), &state.postgres)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => Error::NoClientDataForTopic(topic.clone()),
            e => e.into(),
        })?;
    let project = get_project_by_id(subscriber.project, &state.postgres).await?;

    let Ok(message_bytes) =
        base64::engine::general_purpose::STANDARD.decode(msg.message.to_string())
    else {
        return Err(Error::Other(anyhow!("Failed to decode message")));
    };

    let envelope = Envelope::<EnvelopeType0>::from_bytes(message_bytes)?;

    let sym_key = decode_key(&subscriber.sym_key)?;

    let msg: NotifyRequest<NotifyDelete> = decrypt_message(envelope, &sym_key)?;

    let sub_auth = from_jwt::<SubscriptionDeleteRequestAuth>(&msg.params.delete_auth)?;
    if sub_auth
        .app
        .strip_prefix("did:web:")
        .ok_or(Error::AppNotDidWeb)?
        != project.app_domain
    {
        Err(Error::AppDoesNotMatch)?;
    }

    let (account, siwe_domain) = {
        if sub_auth.shared_claims.act != "notify_delete" {
            return Err(AuthError::InvalidAct)?;
        }

        let Authorization {
            account,
            app,
            domain,
        } = verify_identity(&sub_auth.shared_claims.iss, &sub_auth.ksu, &sub_auth.sub).await?;

        // TODO verify `sub_auth.aud` matches `project_data.identity_keypair`

        if let AuthorizedApp::Limited(app) = app {
            if app != project.app_domain {
                Err(Error::AppSubscriptionsUnauthorized)?;
            }
        }

        (account, domain)
    };

    delete_subscriber(subscriber.id, &state.postgres).await?;

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
            iss: format!("did:key:{identity}"),
            aud: sub_auth.shared_claims.iss,
            act: "notify_delete_response".to_string(),
        },
        sub: format!("did:pkh:{account}"),
        app: format!("did:web:{}", project.app_domain),
    };
    let response_auth = sign_jwt(
        response_message,
        &ed25519_dalek::SigningKey::from_bytes(&decode_key(&project.authentication_private_key)?),
    )?;

    let response = NotifyResponse::<Value> {
        id: msg.id,
        jsonrpc: "2.0".into(),
        result: json!({ "responseAuth": response_auth }), // TODO use structure
    };

    let envelope = Envelope::<EnvelopeType0>::new(&sym_key, response)?;

    let base64_notification = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    let response_topic = sha256::digest(&sym_key);

    state
        .http_relay_client
        .publish(
            response_topic.into(),
            base64_notification,
            NOTIFY_DELETE_RESPONSE_TAG,
            NOTIFY_DELETE_RESPONSE_TTL,
            false,
        )
        .await?;

    update_subscription_watchers(
        account.clone(),
        &project.app_domain,
        &state.postgres,
        &state.http_relay_client.clone(),
        &state.notify_keys.authentication_secret,
        &state.notify_keys.authentication_public,
    )
    .await?;

    Ok(())
}
