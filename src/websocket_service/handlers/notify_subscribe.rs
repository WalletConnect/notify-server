use {
    crate::{
        analytics::notify_client::{NotifyClientMethod, NotifyClientParams},
        auth::{
            add_ttl, from_jwt, sign_jwt, verify_identity, AuthError, Authorization, AuthorizedApp,
            SharedClaims, SubscriptionRequestAuth, SubscriptionResponseAuth,
        },
        error::Error,
        model::helpers::{get_project_by_topic, upsert_subscriber},
        spec::{NOTIFY_SUBSCRIBE_RESPONSE_TAG, NOTIFY_SUBSCRIBE_RESPONSE_TTL},
        state::{AppState, WebhookNotificationEvent},
        types::{Envelope, EnvelopeType0, EnvelopeType1},
        websocket_service::{
            decode_key, derive_key,
            handlers::{decrypt_message, notify_watch_subscriptions::update_subscription_watchers},
            NotifyRequest, NotifyResponse, NotifySubscribe,
        },
        Result,
    },
    base64::Engine,
    chrono::Utc,
    relay_rpc::domain::{DecodedClientId, Topic},
    serde_json::{json, Value},
    std::{collections::HashSet, sync::Arc, time::Duration},
    tracing::{info, instrument},
    x25519_dalek::StaticSecret,
};

// TODO test idempotency (create subscriber a second time for the same account)
#[instrument(name = "wc_notifySubscribe", skip_all)]
pub async fn handle(
    msg: relay_client::websocket::PublishedMessage,
    state: &Arc<AppState>,
    client: &Arc<relay_client::websocket::Client>,
) -> Result<()> {
    let topic = msg.topic;

    let project = get_project_by_topic(topic.clone(), &state.postgres)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => Error::NoProjectDataForTopic(topic.clone()),
            e => e.into(),
        })?;

    let envelope = Envelope::<EnvelopeType1>::from_bytes(
        base64::engine::general_purpose::STANDARD.decode(msg.message.to_string())?,
    )?;

    let client_pubkey = envelope.pubkey();
    let client_pubkey = x25519_dalek::PublicKey::from(client_pubkey);

    let sym_key = derive_key(
        &client_pubkey,
        &x25519_dalek::StaticSecret::from(decode_key(&project.subscribe_private_key)?),
    )?;
    let response_topic = sha256::digest(&sym_key);
    info!("response_topic: {response_topic}");

    let msg: NotifyRequest<NotifySubscribe> = decrypt_message(envelope, &sym_key)?;
    let id = msg.id;

    let sub_auth = from_jwt::<SubscriptionRequestAuth>(&msg.params.subscription_auth)?;
    if sub_auth
        .app
        .strip_prefix("did:web:")
        .ok_or(Error::AppNotDidWeb)?
        != project.app_domain
    {
        Err(Error::AppDoesNotMatch)?;
    }

    let account = {
        if sub_auth.shared_claims.act != "notify_subscription" {
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

        // TODO merge code with integration.rs#verify_jwt()
        //      - put desired `iss` value as an argument to make sure we verify it

        account
    };

    let secret = StaticSecret::random_from_rng(chacha20poly1305::aead::OsRng);

    let identity = DecodedClientId(decode_key(&project.authentication_public_key)?);

    let now = Utc::now();
    let response_message = SubscriptionResponseAuth {
        shared_claims: SharedClaims {
            iat: now.timestamp() as u64,
            exp: add_ttl(now, NOTIFY_SUBSCRIBE_RESPONSE_TTL).timestamp() as u64,
            iss: format!("did:key:{identity}"),
            aud: sub_auth.shared_claims.iss,
            act: "notify_subscription_response".to_string(),
        },
        sub: format!("did:pkh:{account}"),
        app: format!("did:web:{}", project.app_domain),
    };
    let response_auth = sign_jwt(
        response_message,
        &ed25519_dalek::SigningKey::from_bytes(&decode_key(&project.authentication_private_key)?),
    )?;

    let response = NotifyResponse::<Value> {
        id,
        jsonrpc: "2.0".into(),
        result: json!({ "responseAuth": response_auth }), // TODO use structure
    };

    let notify_key = derive_key(&client_pubkey, &secret)?;

    let envelope = Envelope::<EnvelopeType0>::new(&sym_key, response)?;

    let base64_notification = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    let scope = sub_auth
        .scp
        .split(' ')
        .map(|s| s.to_owned())
        .collect::<HashSet<_>>();

    let notify_topic: Topic = sha256::digest(&notify_key).into();

    let project_id = project.project_id;
    info!(
        "Registering account: {account} with topic: {notify_topic} at project: {project_id}. \
         Scope: {scope:?}. RPC ID: {id:?}",
    );

    let subscriber_id = upsert_subscriber(
        project.id,
        account.clone(),
        scope.clone(),
        &notify_key,
        notify_topic.clone(),
        &state.postgres,
    )
    .await?;

    // TODO do in same transaction as upsert_subscriber()
    state
        .notify_webhook(
            project_id.as_ref(),
            WebhookNotificationEvent::Subscribed,
            account.as_ref(),
        )
        .await?;

    state.wsclient.subscribe(notify_topic.clone()).await?;

    state.analytics.client(NotifyClientParams {
        pk: subscriber_id.to_string(),
        method: NotifyClientMethod::Subscribe,
        project_id: project_id.to_string(),
        account: account.to_string(),
        topic: topic.to_string(),
        notify_topic: notify_topic.to_string(),
        old_scope: "".to_owned(),
        new_scope: scope.into_iter().collect::<Vec<_>>().join(","),
    });

    // Send noop to extend ttl of relay's mapping
    info!("publishing noop to notify_topic: {notify_topic}");
    client
        .publish(notify_topic, "", 4050, Duration::from_secs(300), false)
        .await?;

    info!("publishing subscribe response to topic: {response_topic}");
    client
        .publish(
            response_topic.into(),
            base64_notification,
            NOTIFY_SUBSCRIBE_RESPONSE_TAG,
            NOTIFY_SUBSCRIBE_RESPONSE_TTL,
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
