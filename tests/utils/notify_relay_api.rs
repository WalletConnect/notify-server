use {
    super::{
        encode_auth,
        relay_api::{publish_jwt_message, TopicEncrptionScheme, TopicEncryptionSchemeAsymetric},
        RelayClient,
    },
    crate::utils::{
        http_api::get_notify_did_json,
        relay_api::{decode_message, decode_response_message},
        verify_jwt, JWT_LEEWAY,
    },
    notify_server::{
        auth::{
            from_jwt, test_utils::IdentityKeyDetails, DidWeb, NotifyServerSubscription,
            SubscriptionRequestAuth, SubscriptionResponseAuth,
            WatchSubscriptionsChangedRequestAuth, WatchSubscriptionsChangedResponseAuth,
            WatchSubscriptionsRequestAuth, WatchSubscriptionsResponseAuth,
        },
        model::types::AccountId,
        notify_message::NotifyMessage,
        rpc::{
            derive_key, JsonRpcRequest, JsonRpcResponse, JsonRpcResponseError, NotifyMessageAuth,
            NotifySubscribe, NotifySubscriptionsChanged, NotifyWatchSubscriptions, ResponseAuth,
        },
        spec::{
            NOTIFY_MESSAGE_ACT, NOTIFY_MESSAGE_METHOD, NOTIFY_MESSAGE_TAG, NOTIFY_SUBSCRIBE_ACT,
            NOTIFY_SUBSCRIBE_METHOD, NOTIFY_SUBSCRIBE_RESPONSE_ACT, NOTIFY_SUBSCRIBE_RESPONSE_TAG,
            NOTIFY_SUBSCRIBE_TAG, NOTIFY_SUBSCRIBE_TTL, NOTIFY_SUBSCRIPTIONS_CHANGED_ACT,
            NOTIFY_SUBSCRIPTIONS_CHANGED_METHOD, NOTIFY_SUBSCRIPTIONS_CHANGED_RESPONE_ACT,
            NOTIFY_SUBSCRIPTIONS_CHANGED_RESPONSE_TAG, NOTIFY_SUBSCRIPTIONS_CHANGED_RESPONSE_TTL,
            NOTIFY_SUBSCRIPTIONS_CHANGED_TAG, NOTIFY_WATCH_SUBSCRIPTIONS_ACT,
            NOTIFY_WATCH_SUBSCRIPTIONS_METHOD, NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_ACT,
            NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_TAG, NOTIFY_WATCH_SUBSCRIPTIONS_TAG,
            NOTIFY_WATCH_SUBSCRIPTIONS_TTL,
        },
        utils::{is_same_address, topic_from_key},
    },
    rand_chacha::rand_core::OsRng,
    relay_rpc::{
        auth::ed25519_dalek::VerifyingKey,
        domain::{DecodedClientId, MessageId},
    },
    std::collections::HashSet,
    url::Url,
    uuid::Uuid,
    x25519_dalek::{PublicKey, StaticSecret},
};

async fn publish_watch_subscriptions_request(
    relay_client: &mut RelayClient,
    account: &AccountId,
    client_id: &DecodedClientId,
    identity_key_details: &IdentityKeyDetails,
    encryption_details: TopicEncryptionSchemeAsymetric,
    app: Option<DidWeb>,
) {
    publish_jwt_message(
        relay_client,
        client_id,
        identity_key_details,
        &TopicEncrptionScheme::Asymetric(encryption_details),
        NOTIFY_WATCH_SUBSCRIPTIONS_TAG,
        NOTIFY_WATCH_SUBSCRIPTIONS_TTL,
        NOTIFY_WATCH_SUBSCRIPTIONS_ACT,
        None,
        |shared_claims| {
            serde_json::to_value(JsonRpcRequest::new(
                NOTIFY_WATCH_SUBSCRIPTIONS_METHOD,
                NotifyWatchSubscriptions {
                    watch_subscriptions_auth: encode_auth(
                        &WatchSubscriptionsRequestAuth {
                            shared_claims,
                            ksu: identity_key_details.keys_server_url.to_string(),
                            sub: account.to_did_pkh(),
                            app,
                        },
                        &identity_key_details.signing_key,
                    ),
                },
            ))
            .unwrap()
        },
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn watch_subscriptions(
    relay_client: &mut RelayClient,
    notify_server_url: Url,
    identity_key_details: &IdentityKeyDetails,
    app_domain: Option<DidWeb>,
    account: &AccountId,
) -> Result<(Vec<NotifyServerSubscription>, [u8; 32], DecodedClientId), JsonRpcResponseError<String>>
{
    let (key_agreement_key, client_id) = get_notify_did_json(&notify_server_url).await;

    let secret = StaticSecret::random_from_rng(OsRng);
    let public = PublicKey::from(&secret);

    let response_topic_key = derive_key(&key_agreement_key, &secret).unwrap();
    let response_topic = topic_from_key(&response_topic_key);

    publish_watch_subscriptions_request(
        relay_client,
        account,
        &client_id,
        identity_key_details,
        TopicEncryptionSchemeAsymetric {
            client_private: secret,
            client_public: public,
            server_public: key_agreement_key,
        },
        app_domain,
    )
    .await;

    relay_client.subscribe(response_topic.clone()).await;

    let msg = relay_client
        .accept_message(NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_TAG, &response_topic)
        .await;

    let (_id, auth) =
        decode_response_message::<WatchSubscriptionsResponseAuth>(msg, &response_topic_key)?;
    assert_eq!(
        auth.shared_claims.act,
        NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_ACT
    );
    assert_eq!(auth.shared_claims.iss, client_id.to_did_key());
    assert_eq!(
        auth.shared_claims.aud,
        identity_key_details.client_id.to_did_key()
    );
    assert_eq!(auth.sub, account.to_did_pkh());

    Ok((auth.sbs, response_topic_key, client_id))
}

async fn publish_subscriptions_changed_response(
    relay_client: &mut RelayClient,
    did_pkh: &AccountId,
    client_id: &DecodedClientId,
    identity_key_details: &IdentityKeyDetails,
    sym_key: [u8; 32],
    id: MessageId,
) {
    publish_jwt_message(
        relay_client,
        client_id,
        identity_key_details,
        &TopicEncrptionScheme::Symetric(sym_key),
        NOTIFY_SUBSCRIPTIONS_CHANGED_RESPONSE_TAG,
        NOTIFY_SUBSCRIPTIONS_CHANGED_RESPONSE_TTL,
        NOTIFY_SUBSCRIPTIONS_CHANGED_RESPONE_ACT,
        None,
        |shared_claims| {
            serde_json::to_value(JsonRpcResponse::new(
                id,
                ResponseAuth {
                    response_auth: encode_auth(
                        &WatchSubscriptionsChangedResponseAuth {
                            shared_claims,
                            ksu: identity_key_details.keys_server_url.to_string(),
                            sub: did_pkh.to_did_pkh(),
                        },
                        &identity_key_details.signing_key,
                    ),
                },
            ))
            .unwrap()
        },
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn accept_watch_subscriptions_changed(
    relay_client: &mut RelayClient,
    notify_server_client_id: &DecodedClientId,
    identity_key_details: &IdentityKeyDetails,
    account: &AccountId,
    watch_topic_key: [u8; 32],
) -> Result<Vec<NotifyServerSubscription>, JsonRpcResponseError<String>> {
    let msg = relay_client
        .accept_message(
            NOTIFY_SUBSCRIPTIONS_CHANGED_TAG,
            &topic_from_key(&watch_topic_key),
        )
        .await;

    let request =
        decode_message::<JsonRpcRequest<NotifySubscriptionsChanged>>(msg, &watch_topic_key)?;
    assert_eq!(request.method, NOTIFY_SUBSCRIPTIONS_CHANGED_METHOD);
    let auth = from_jwt::<WatchSubscriptionsChangedRequestAuth>(
        &request.params.subscriptions_changed_auth,
    )
    .unwrap();

    assert_eq!(auth.shared_claims.act, NOTIFY_SUBSCRIPTIONS_CHANGED_ACT);
    assert_eq!(auth.shared_claims.iss, notify_server_client_id.to_did_key());
    assert_eq!(
        auth.shared_claims.aud,
        identity_key_details.client_id.to_did_key()
    );
    assert_eq!(auth.sub, account.to_did_pkh());

    publish_subscriptions_changed_response(
        relay_client,
        account,
        notify_server_client_id,
        identity_key_details,
        watch_topic_key,
        request.id,
    )
    .await;

    Ok(auth.sbs)
}

#[allow(clippy::too_many_arguments)]
async fn publish_subscribe_request(
    relay_client: &mut RelayClient,
    did_pkh: String,
    client_id: &DecodedClientId,
    identity_key_details: &IdentityKeyDetails,
    encryption_details: TopicEncryptionSchemeAsymetric,
    app: DidWeb,
    notification_types: HashSet<Uuid>,
    mjv: String,
) {
    publish_jwt_message(
        relay_client,
        client_id,
        identity_key_details,
        &TopicEncrptionScheme::Asymetric(encryption_details),
        NOTIFY_SUBSCRIBE_TAG,
        NOTIFY_SUBSCRIBE_TTL,
        NOTIFY_SUBSCRIBE_ACT,
        Some(mjv),
        |shared_claims| {
            serde_json::to_value(JsonRpcRequest::new(
                NOTIFY_SUBSCRIBE_METHOD,
                NotifySubscribe {
                    subscription_auth: encode_auth(
                        &SubscriptionRequestAuth {
                            shared_claims,
                            ksu: identity_key_details.keys_server_url.to_string(),
                            sub: did_pkh.clone(),
                            scp: notification_types
                                .iter()
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                                .join(" "),
                            app,
                        },
                        &identity_key_details.signing_key,
                    ),
                },
            ))
            .unwrap()
        },
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn subscribe(
    relay_client: &mut RelayClient,
    account: &AccountId,
    identity_key_details: &IdentityKeyDetails,
    app_key_agreement_key: x25519_dalek::PublicKey,
    app_client_id: &DecodedClientId,
    app: DidWeb,
    notification_types: HashSet<Uuid>,
) -> Result<(), JsonRpcResponseError<String>> {
    let _subs = subscribe_with_mjv(
        relay_client,
        account,
        identity_key_details,
        app_key_agreement_key,
        app_client_id,
        app,
        notification_types,
        "0".to_owned(),
    )
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn subscribe_with_mjv(
    relay_client: &mut RelayClient,
    account: &AccountId,
    identity_key_details: &IdentityKeyDetails,
    app_key_agreement_key: x25519_dalek::PublicKey,
    app_client_id: &DecodedClientId,
    app: DidWeb,
    notification_types: HashSet<Uuid>,
    mjv: String,
) -> Result<Vec<NotifyServerSubscription>, JsonRpcResponseError<String>> {
    let secret = StaticSecret::random_from_rng(OsRng);
    let public = PublicKey::from(&secret);
    let response_topic_key = derive_key(&app_key_agreement_key, &secret).unwrap();
    let response_topic = topic_from_key(&response_topic_key);

    publish_subscribe_request(
        relay_client,
        account.to_did_pkh(),
        app_client_id,
        identity_key_details,
        TopicEncryptionSchemeAsymetric {
            client_private: secret,
            client_public: public,
            server_public: app_key_agreement_key,
        },
        app,
        notification_types,
        mjv,
    )
    .await;

    // https://walletconnect.slack.com/archives/C03SMNKLPU0/p1704449850496039?thread_ts=1703984667.223199&cid=C03SMNKLPU0
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    relay_client.subscribe(response_topic.clone()).await;

    let msg = relay_client
        .accept_message(NOTIFY_SUBSCRIBE_RESPONSE_TAG, &response_topic)
        .await;

    let (_id, auth) =
        decode_response_message::<SubscriptionResponseAuth>(msg, &response_topic_key)?;
    assert_eq!(auth.shared_claims.act, NOTIFY_SUBSCRIBE_RESPONSE_ACT);
    assert_eq!(auth.shared_claims.iss, app_client_id.to_did_key());
    assert_eq!(
        auth.shared_claims.aud,
        identity_key_details.client_id.to_did_key()
    );
    assert_eq!(auth.sub, account.to_did_pkh());

    Ok(auth.sbs)
}

#[allow(clippy::too_many_arguments)]
pub async fn accept_notify_message(
    client: &mut RelayClient,
    account: &AccountId,
    app_authentication: &VerifyingKey,
    app_client_id: &DecodedClientId,
    app_domain: &DidWeb,
    notify_key: &[u8; 32],
) -> Result<(MessageId, NotifyMessage), JsonRpcResponseError<String>> {
    let msg = client
        .accept_message(NOTIFY_MESSAGE_TAG, &topic_from_key(notify_key))
        .await;

    let request = decode_message::<JsonRpcRequest<NotifyMessageAuth>>(msg, notify_key)?;
    assert_eq!(request.method, NOTIFY_MESSAGE_METHOD);

    let claims = verify_jwt(&request.params.message_auth, app_authentication).unwrap();

    assert_eq!(claims.iss, app_client_id.to_did_key());
    assert_eq!(claims.sub, account.to_did_pkh());
    assert!(claims.iat < chrono::Utc::now().timestamp() + JWT_LEEWAY); // TODO remove leeway
    assert!(claims.exp > chrono::Utc::now().timestamp() - JWT_LEEWAY); // TODO remove leeway
    assert_eq!(&claims.app, app_domain);
    assert_eq!(claims.act, NOTIFY_MESSAGE_ACT);
    assert!(is_same_address(
        &AccountId::from_did_pkh(&claims.sub).unwrap(),
        account
    ));

    Ok((request.id, claims))
}
