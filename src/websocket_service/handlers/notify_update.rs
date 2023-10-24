use {
    super::notify_watch_subscriptions::update_subscription_watchers,
    crate::{
        analytics::subscriber_update::{NotifyClientMethod, SubscriberUpdateParams},
        auth::{
            add_ttl, from_jwt, sign_jwt, verify_identity, AuthError, Authorization, AuthorizedApp,
            SharedClaims, SubscriptionUpdateRequestAuth, SubscriptionUpdateResponseAuth,
        },
        error::Error,
        model::helpers::{get_project_by_id, get_subscriber_by_topic, update_subscriber},
        spec::{NOTIFY_UPDATE_RESPONSE_TAG, NOTIFY_UPDATE_RESPONSE_TTL},
        state::AppState,
        types::{Envelope, EnvelopeType0},
        websocket_service::{
            decode_key, handlers::decrypt_message, NotifyRequest, NotifyResponse, NotifyUpdate,
        },
        Result,
    },
    base64::Engine,
    chrono::Utc,
    relay_rpc::domain::DecodedClientId,
    serde_json::{json, Value},
    std::{collections::HashSet, sync::Arc},
};

// TODO test idempotency
pub async fn handle(
    msg: relay_client::websocket::PublishedMessage,
    state: &Arc<AppState>,
    client: &Arc<relay_client::websocket::Client>,
) -> Result<()> {
    let topic = msg.topic;

    // TODO combine these two SQL queries
    let subscriber = get_subscriber_by_topic(topic.clone(), &state.postgres)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => Error::NoClientDataForTopic(topic.clone()),
            e => e.into(),
        })?;
    let project = get_project_by_id(subscriber.project, &state.postgres).await?;

    let envelope = Envelope::<EnvelopeType0>::from_bytes(
        base64::engine::general_purpose::STANDARD.decode(msg.message.to_string())?,
    )?;

    let sym_key = decode_key(&subscriber.sym_key)?;

    let msg: NotifyRequest<NotifyUpdate> = decrypt_message(envelope, &sym_key)?;

    let sub_auth = from_jwt::<SubscriptionUpdateRequestAuth>(&msg.params.update_auth)?;
    if sub_auth
        .app
        .strip_prefix("did:web:")
        .ok_or(Error::AppNotDidWeb)?
        != project.app_domain
    {
        Err(Error::AppDoesNotMatch)?;
    }

    let account = {
        if sub_auth.shared_claims.act != "notify_update" {
            return Err(AuthError::InvalidAct)?;
        }

        let Authorization { account, app } =
            verify_identity(&sub_auth.shared_claims.iss, &sub_auth.ksu, &sub_auth.sub).await?;

        // TODO verify `sub_auth.aud` matches `project_data.identity_keypair`

        if let AuthorizedApp::Limited(app) = app {
            if app != project.app_domain {
                Err(Error::AppSubscriptionsUnauthorized)?;
            }
        }

        account
    };

    let old_scope = subscriber.scope;
    let new_scope = sub_auth
        .scp
        .split(' ')
        .map(|s| s.to_owned())
        .collect::<HashSet<_>>();

    let subscriber = update_subscriber(
        project.id,
        account.clone(),
        new_scope.clone(),
        &state.postgres,
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
        project_id: project.project_id,
        pk: subscriber.id,
        account: account.clone(),
        method: NotifyClientMethod::Update,
        old_scope: old_scope.into_iter().map(Into::into).collect(),
        new_scope: new_scope.into_iter().map(Into::into).collect(),
        notify_topic: subscriber.topic.clone(),
        topic,
    });

    let identity = DecodedClientId(decode_key(&project.authentication_public_key)?);

    let now = Utc::now();
    let response_message = SubscriptionUpdateResponseAuth {
        shared_claims: SharedClaims {
            iat: now.timestamp() as u64,
            exp: add_ttl(now, NOTIFY_UPDATE_RESPONSE_TTL).timestamp() as u64,
            iss: format!("did:key:{identity}"),
            aud: sub_auth.shared_claims.iss,
            act: "notify_update_response".to_string(),
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

    client
        .publish(
            response_topic.into(),
            base64_notification,
            NOTIFY_UPDATE_RESPONSE_TAG,
            NOTIFY_UPDATE_RESPONSE_TTL,
            false,
        )
        .await?;

    update_subscription_watchers(
        account,
        &project.app_domain,
        &state.postgres,
        client.as_ref(),
        &state.notify_keys.authentication_secret,
        &state.notify_keys.authentication_public,
    )
    .await?;

    Ok(())
}
