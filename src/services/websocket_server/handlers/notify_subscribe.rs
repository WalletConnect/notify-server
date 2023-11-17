use {
    crate::{
        analytics::subscriber_update::{NotifyClientMethod, SubscriberUpdateParams},
        auth::{
            add_ttl, from_jwt, sign_jwt, verify_identity, AuthError, Authorization, AuthorizedApp,
            SharedClaims, SubscriptionRequestAuth, SubscriptionResponseAuth,
        },
        error::Error,
        model::helpers::{get_project_by_topic, upsert_subscriber},
        publish_relay_message::publish_relay_message,
        services::websocket_server::{
            decode_key, derive_key,
            handlers::{decrypt_message, notify_watch_subscriptions::update_subscription_watchers},
            NotifyRequest, NotifyResponse, NotifySubscribe,
        },
        spec::{NOTIFY_NOOP, NOTIFY_SUBSCRIBE_RESPONSE_TAG, NOTIFY_SUBSCRIBE_RESPONSE_TTL},
        state::{AppState, WebhookNotificationEvent},
        types::{parse_scope, Envelope, EnvelopeType0, EnvelopeType1},
        Result,
    },
    base64::Engine,
    chrono::Utc,
    relay_client::websocket::PublishedMessage,
    relay_rpc::{
        domain::{DecodedClientId, Topic},
        rpc::{Publish, JSON_RPC_VERSION_STR},
    },
    serde_json::{json, Value},
    std::collections::HashSet,
    tracing::{info, instrument},
    x25519_dalek::StaticSecret,
};

// TODO test idempotency (create subscriber a second time for the same account)
#[instrument(name = "wc_notifySubscribe", skip_all)]
pub async fn handle(msg: PublishedMessage, state: &AppState) -> Result<()> {
    let topic = msg.topic;

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
    info!(
        "sub_auth.shared_claims.iss: {:?}",
        sub_auth.shared_claims.iss
    );
    if sub_auth
        .app
        .strip_prefix("did:web:")
        .ok_or(Error::AppNotDidWeb)?
        != project.app_domain
    {
        Err(Error::AppDoesNotMatch)?;
    }

    let (account, siwe_domain) = {
        if sub_auth.shared_claims.act != "notify_subscription" {
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
            iss: format!("did:key:{identity}"),
            aud: sub_auth.shared_claims.iss.clone(),
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
        jsonrpc: JSON_RPC_VERSION_STR.to_owned(),
        result: json!({ "responseAuth": response_auth }), // TODO use structure
    };

    let notify_key = derive_key(&client_pubkey, &secret)?;

    let envelope = Envelope::<EnvelopeType0>::new(&sym_key, response)?;

    let base64_notification = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    let scope = parse_scope(&sub_auth.scp)?;

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
        state.metrics.as_ref(),
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

    state
        .relay_ws_client
        .subscribe(notify_topic.clone())
        .await?;

    state.analytics.client(SubscriberUpdateParams {
        project_pk: project.id,
        project_id,
        pk: subscriber_id,
        account: account.clone(),
        updated_by_iss: sub_auth.shared_claims.iss.into(),
        updated_by_domain: siwe_domain,
        method: NotifyClientMethod::Subscribe,
        old_scope: HashSet::new(),
        new_scope: scope,
        notification_topic: notify_topic.clone(),
        topic,
    });

    // Send noop to extend ttl of relay's mapping
    info!("publishing noop to notify_topic: {notify_topic}");
    publish_relay_message(
        &state.relay_http_client,
        &Publish {
            topic: notify_topic,
            message: "".into(),
            tag: NOTIFY_NOOP,
            ttl_secs: 300,
            prompt: false,
        },
        state.metrics.as_ref(),
    )
    .await?;

    info!("publishing subscribe response to topic: {response_topic}");
    publish_relay_message(
        &state.relay_http_client,
        &Publish {
            topic: response_topic.into(),
            message: base64_notification.into(),
            tag: NOTIFY_SUBSCRIBE_RESPONSE_TAG,
            ttl_secs: NOTIFY_SUBSCRIBE_RESPONSE_TTL.as_secs() as u32,
            prompt: false,
        },
        state.metrics.as_ref(),
    )
    .await?;

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
