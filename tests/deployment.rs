use {
    crate::utils::{
        create_client, encode_auth, generate_account, verify_jwt, UnregisterIdentityRequestAuth,
        JWT_LEEWAY, RELAY_MESSAGE_DELIVERY_TIMEOUT,
    },
    base64::Engine,
    chacha20poly1305::{
        aead::{generic_array::GenericArray, Aead, OsRng},
        ChaCha20Poly1305, KeyInit,
    },
    chrono::Utc,
    data_encoding::BASE64URL,
    ed25519_dalek::{SigningKey, VerifyingKey},
    hyper::StatusCode,
    notify_server::{
        auth::{
            add_ttl, from_jwt, DidWeb, NotifyServerSubscription, SharedClaims,
            SubscriptionDeleteRequestAuth, SubscriptionDeleteResponseAuth, SubscriptionRequestAuth,
            SubscriptionResponseAuth, SubscriptionUpdateRequestAuth,
            SubscriptionUpdateResponseAuth, WatchSubscriptionsChangedRequestAuth,
            WatchSubscriptionsRequestAuth, WatchSubscriptionsResponseAuth, STATEMENT_THIS_DOMAIN,
        },
        jsonrpc::NotifyPayload,
        publish_relay_message::subscribe_relay_topic,
        services::{
            public_http_server::handlers::{
                notify_v0::NotifyBody,
                subscribe_topic::{SubscribeTopicRequestBody, SubscribeTopicResponseBody},
            },
            websocket_server::{
                decode_key, derive_key, relay_ws_client::RelayClientEvent, NotifyRequest,
                NotifyResponse, NotifyWatchSubscriptions,
            },
        },
        spec::{
            NOTIFY_DELETE_ACT, NOTIFY_DELETE_METHOD, NOTIFY_DELETE_RESPONSE_ACT,
            NOTIFY_DELETE_RESPONSE_TAG, NOTIFY_DELETE_TAG, NOTIFY_DELETE_TTL, NOTIFY_MESSAGE_ACT,
            NOTIFY_MESSAGE_TAG, NOTIFY_NOOP_TAG, NOTIFY_SUBSCRIBE_ACT, NOTIFY_SUBSCRIBE_METHOD,
            NOTIFY_SUBSCRIBE_RESPONSE_ACT, NOTIFY_SUBSCRIBE_RESPONSE_TAG, NOTIFY_SUBSCRIBE_TAG,
            NOTIFY_SUBSCRIBE_TTL, NOTIFY_SUBSCRIPTIONS_CHANGED_ACT,
            NOTIFY_SUBSCRIPTIONS_CHANGED_TAG, NOTIFY_UPDATE_ACT, NOTIFY_UPDATE_METHOD,
            NOTIFY_UPDATE_RESPONSE_ACT, NOTIFY_UPDATE_RESPONSE_TAG, NOTIFY_UPDATE_TAG,
            NOTIFY_UPDATE_TTL, NOTIFY_WATCH_SUBSCRIPTIONS_ACT, NOTIFY_WATCH_SUBSCRIPTIONS_METHOD,
            NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_ACT, NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_TAG,
            NOTIFY_WATCH_SUBSCRIPTIONS_TAG, NOTIFY_WATCH_SUBSCRIPTIONS_TTL,
        },
        types::{encode_scope, Envelope, EnvelopeType0, EnvelopeType1, Notification},
        utils::topic_from_key,
    },
    rand::{rngs::StdRng, SeedableRng},
    relay_rpc::{
        auth::{
            cacao::{self, signature::Eip191},
            ed25519_dalek::Keypair,
        },
        domain::DecodedClientId,
        rpc::msg_id::get_message_id,
    },
    serde_json::json,
    sha2::Digest,
    sha3::Keccak256,
    std::{collections::HashSet, env},
    tokio::sync::broadcast::Receiver,
    url::Url,
    uuid::Uuid,
    x25519_dalek::{PublicKey, StaticSecret},
};

mod utils;

// These tests test full integration of the notify server with other services. It is intended to test prod and staging environments in CD, but is also used to test locally.
// To run these tests locally, initialize `.env` with the "LOCAL deployment tests configuration" and "LOCAL or PROD deployment tests configuration"
//   and run `just run` and `just test-integration`.
//   To simplify development, `just devloop` can be used instead which automatically runs `just run` and also includes all other tests.

// These tests run against the LOCAL environment by default, but this can be changed by specifying the `ENVIRONMENT` variable, for example `ENVIRONMENT=DEV just test-integration`.
// If testing against DEV or STAGING environments, additional variables must be set in `.env` titled "DEV or STAGING deployment tests configuration"

// The Notify Server URL is chosen automatically depending on the chosen environment.
// Depending on the Notify Server chosen:
// - The necessary relay will be used. All relays use prod registry so the same prod project ID can be used.
// - The necessary NOTIFY_*_PROJECT_ID and NOTIFY_*_PROJECT_SECRET variables will be read. STAGING Notify Server uses staging registry, so a different project ID and secret must be used.
//   - To support CD, NOTIFY_PROJECT_ID and NOTIFY_PROJECT_SECRET variables are accepted as fallbacks. However to ease local development different variable names are used primiarly to avoid needing to change `.env` depending on which environment is being tested.
// The staging keys server is always used, to avoid unnecessary load on prod server.

fn get_vars() -> Vars {
    let relay_project_id = env::var("PROJECT_ID").unwrap();
    let keys_server_url = "https://staging.keys.walletconnect.com".parse().unwrap();

    let notify_prod_project_id = || {
        env::var("NOTIFY_PROD_PROJECT_ID")
            .unwrap_or_else(|_| env::var("NOTIFY_PROJECT_ID").unwrap())
    };
    let notify_prod_project_secret = || {
        env::var("NOTIFY_PROD_PROJECT_SECRET")
            .unwrap_or_else(|_| env::var("NOTIFY_PROJECT_SECRET").unwrap())
    };
    let notify_staging_project_id = || {
        env::var("NOTIFY_STAGING_PROJECT_ID")
            .unwrap_or_else(|_| env::var("NOTIFY_PROJECT_ID").unwrap())
    };
    let notify_staging_project_secret = || {
        env::var("NOTIFY_STAGING_PROJECT_SECRET")
            .unwrap_or_else(|_| env::var("NOTIFY_PROJECT_SECRET").unwrap())
    };

    let env = std::env::var("ENVIRONMENT").unwrap_or_else(|_| "LOCAL".to_owned());
    match env.as_str() {
        "PROD" => Vars {
            notify_url: "https://notify.walletconnect.com".to_owned(),
            relay_url: "wss://relay.walletconnect.com".to_owned(),
            relay_project_id,
            notify_project_id: notify_prod_project_id(),
            notify_project_secret: notify_prod_project_secret(),
            keys_server_url,
        },
        "STAGING" => Vars {
            notify_url: "https://staging.notify.walletconnect.com".to_owned(),
            relay_url: "wss://staging.relay.walletconnect.com".to_owned(),
            relay_project_id,
            notify_project_id: notify_staging_project_id(),
            notify_project_secret: notify_staging_project_secret(),
            keys_server_url,
        },
        "DEV" => Vars {
            notify_url: "https://dev.notify.walletconnect.com".to_owned(),
            relay_url: "wss://staging.relay.walletconnect.com".to_owned(),
            relay_project_id,
            notify_project_id: notify_prod_project_id(),
            notify_project_secret: notify_prod_project_secret(),
            keys_server_url,
        },
        "LOCAL" => Vars {
            notify_url: "http://127.0.0.1:3000".to_owned(),
            relay_url: "wss://staging.relay.walletconnect.com".to_owned(),
            relay_project_id,
            notify_project_id: notify_prod_project_id(),
            notify_project_secret: notify_prod_project_secret(),
            keys_server_url,
        },
        e => panic!("Invalid ENVIRONMENT: {}", e),
    }
}

struct Vars {
    notify_url: String,
    relay_url: String,
    relay_project_id: String,
    notify_project_id: String,
    notify_project_secret: String,
    keys_server_url: Url,
}

#[allow(clippy::too_many_arguments)]
async fn watch_subscriptions(
    vars: &Vars,
    app_domain: Option<&str>,
    identity_signing_key: &SigningKey,
    identity_did_key: &str,
    did_pkh: &str,
    relay_ws_client: &relay_client::websocket::Client,
    rx: &mut Receiver<RelayClientEvent>,
) -> (Vec<NotifyServerSubscription>, [u8; 32]) {
    let (key_agreement_key, authentication_key) = {
        let did_json_url = Url::parse(&vars.notify_url)
            .unwrap()
            .join("/.well-known/did.json")
            .unwrap();
        let did_json = reqwest::get(did_json_url)
            .await
            .unwrap()
            .json::<serde_json::Value>() // TODO use struct
            .await
            .unwrap();
        let verification_method = did_json
            .get("verificationMethod")
            .unwrap()
            .as_array()
            .unwrap();
        let key_agreement = verification_method
            .iter()
            .find(|key| {
                key.as_object()
                    .unwrap()
                    .get("id")
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .ends_with("wc-notify-subscribe-key")
            })
            .unwrap()
            .as_object()
            .unwrap()
            .get("publicKeyJwk")
            .unwrap()
            .as_object()
            .unwrap()
            .get("x")
            .unwrap()
            .as_str()
            .unwrap();
        let authentication = verification_method
            .iter()
            .find(|key| {
                key.as_object()
                    .unwrap()
                    .get("id")
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .ends_with("wc-notify-authentication-key")
            })
            .unwrap()
            .as_object()
            .unwrap()
            .get("publicKeyJwk")
            .unwrap()
            .as_object()
            .unwrap()
            .get("x")
            .unwrap()
            .as_str()
            .unwrap();
        let key_agreement: [u8; 32] = BASE64URL
            .decode(key_agreement.as_bytes())
            .unwrap()
            .try_into()
            .unwrap();
        let authentication: [u8; 32] = BASE64URL
            .decode(authentication.as_bytes())
            .unwrap()
            .try_into()
            .unwrap();
        (key_agreement, authentication)
    };

    let now = Utc::now();
    let subscription_auth = WatchSubscriptionsRequestAuth {
        shared_claims: SharedClaims {
            iat: now.timestamp() as u64,
            exp: add_ttl(now, NOTIFY_SUBSCRIBE_TTL).timestamp() as u64,
            iss: identity_did_key.to_owned(),
            act: NOTIFY_WATCH_SUBSCRIPTIONS_ACT.to_owned(),
            aud: DecodedClientId(authentication_key).to_did_key(),
            mjv: "0".to_owned(),
        },
        ksu: vars.keys_server_url.to_string(),
        sub: did_pkh.to_owned(),
        app: app_domain.map(|domain| DidWeb::from_domain(domain.to_owned())),
    };

    let message = NotifyRequest::new(
        NOTIFY_WATCH_SUBSCRIPTIONS_METHOD,
        NotifyWatchSubscriptions {
            watch_subscriptions_auth: encode_auth(&subscription_auth, identity_signing_key),
        },
    );

    let secret = StaticSecret::random_from_rng(OsRng);
    let public = PublicKey::from(&secret);

    let response_topic_key =
        derive_key(&x25519_dalek::PublicKey::from(key_agreement_key), &secret).unwrap();
    let response_topic = topic_from_key(&response_topic_key);

    let envelope = Envelope::<EnvelopeType1>::new(
        &response_topic_key,
        serde_json::to_value(message).unwrap(),
        *public.as_bytes(),
    )
    .unwrap();
    let message = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    let watch_subscriptions_topic = topic_from_key(&key_agreement_key);
    relay_ws_client
        .publish(
            watch_subscriptions_topic,
            message,
            NOTIFY_WATCH_SUBSCRIPTIONS_TAG,
            NOTIFY_WATCH_SUBSCRIPTIONS_TTL,
            false,
        )
        .await
        .unwrap();

    subscribe_relay_topic(relay_ws_client, &response_topic, None)
        .await
        .unwrap();

    let resp = rx.recv().await.unwrap();

    let RelayClientEvent::Message(msg) = resp else {
        panic!("Expected message, got {:?}", resp);
    };
    assert_eq!(msg.tag, NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_TAG);

    let Envelope::<EnvelopeType0> { sealbox, iv, .. } = Envelope::<EnvelopeType0>::from_bytes(
        base64::engine::general_purpose::STANDARD
            .decode(msg.message.as_bytes())
            .unwrap(),
    )
    .unwrap();
    let decrypted_response = ChaCha20Poly1305::new(GenericArray::from_slice(&response_topic_key))
        .decrypt(&iv.into(), chacha20poly1305::aead::Payload::from(&*sealbox))
        .unwrap();
    let response: NotifyResponse<serde_json::Value> =
        serde_json::from_slice(&decrypted_response).unwrap();

    println!(
        "received watch_subscriptions_response with id msg.id {} and message_id {} and RPC ID {}",
        msg.message_id,
        get_message_id(&msg.message),
        response.id,
    );

    let response_auth = response
        .result
        .get("responseAuth") // TODO use structure
        .unwrap()
        .as_str()
        .unwrap();
    let auth = from_jwt::<WatchSubscriptionsResponseAuth>(response_auth).unwrap();
    assert_eq!(
        auth.shared_claims.act,
        NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_ACT
    );
    assert_eq!(
        auth.shared_claims.iss,
        DecodedClientId(authentication_key).to_did_key()
    );

    (auth.sbs, response_topic_key)
}

async fn run_test(statement: String, watch_subscriptions_all_domains: bool) {
    let vars = get_vars();

    let (identity_signing_key, identity_did_key) = {
        let keypair = Keypair::generate(&mut StdRng::from_entropy());
        let signing_key = SigningKey::from_bytes(keypair.secret_key().as_bytes());
        let client_id = DecodedClientId::from_key(&keypair.public_key());
        let client_did_key = client_id.to_did_key();
        (signing_key, client_did_key)
    };

    let (account_signing_key, account) = generate_account();
    let did_pkh = account.to_did_pkh();

    let app_domain = format!("{}.walletconnect.com", vars.notify_project_id);

    // Register identity key with keys server
    {
        let mut cacao = cacao::Cacao {
            h: cacao::header::Header {
                t: "eip4361".to_owned(),
            },
            p: cacao::payload::Payload {
                domain: app_domain.to_owned(),
                iss: did_pkh.clone(),
                statement: Some(statement),
                aud: identity_did_key.clone(),
                version: cacao::Version::V1,
                nonce: "xxxx".to_owned(), // TODO
                iat: Utc::now().to_rfc3339(),
                exp: None,
                nbf: None,
                request_id: None,
                resources: Some(vec![vars.keys_server_url.to_string()]),
            },
            s: cacao::signature::Signature {
                t: "".to_owned(),
                s: "".to_owned(),
            },
        };
        let (signature, recovery): (k256::ecdsa::Signature, _) = account_signing_key
            .sign_digest_recoverable(Keccak256::new_with_prefix(
                Eip191.eip191_bytes(&cacao.siwe_message().unwrap()),
            ))
            .unwrap();
        let cacao_signature = [&signature.to_bytes()[..], &[recovery.to_byte()]].concat();
        cacao.s.t = "eip191".to_owned();
        cacao.s.s = hex::encode(cacao_signature);
        cacao.verify().unwrap();

        let response = reqwest::Client::builder()
            .build()
            .unwrap()
            .post(vars.keys_server_url.join("/identity").unwrap())
            .header("Content-Type", "application/json")
            .body(serde_json::to_string(&json!({"cacao": cacao})).unwrap())
            .send()
            .await
            .unwrap();
        let status = response.status();
        assert!(status.is_success());
    }

    // ==== watchSubscriptions ====
    // {
    //     let (relay_ws_client, mut rx) = create_client(&relay_url, &relay_project_id, &notify_url).await;

    //     let (subs, _) = watch_subscriptions(
    //         app_domain,
    //         &notify_url,
    //         &identity_signing_key,
    //         &identity_did_key,
    //         &did_pkh,
    //         &relay_ws_client,
    //         &mut rx,
    //     )
    //     .await;

    //     assert!(subs.is_empty());
    // }

    let (relay_ws_client, mut rx) = create_client(
        vars.relay_url.parse().unwrap(),
        vars.relay_project_id.clone().into(),
        vars.notify_url.parse().unwrap(),
    )
    .await;

    // ==== subscribe topic ====

    // Register project - generating subscribe topic
    let subscribe_topic_response = reqwest::Client::new()
        .post(format!(
            "{}/{}/subscribe-topic",
            &vars.notify_url, &vars.notify_project_id
        ))
        .bearer_auth(&vars.notify_project_secret)
        .json(&SubscribeTopicRequestBody {
            app_domain: app_domain.clone().into(),
        })
        .send()
        .await
        .unwrap();
    assert_eq!(subscribe_topic_response.status(), StatusCode::OK);
    let subscribe_topic_response_body = subscribe_topic_response
        .json::<SubscribeTopicResponseBody>()
        .await
        .unwrap();

    let watch_topic_key = {
        let (subs, watch_topic_key) = watch_subscriptions(
            &vars,
            if watch_subscriptions_all_domains {
                None
            } else {
                Some(&app_domain)
            },
            &identity_signing_key,
            &identity_did_key,
            &did_pkh,
            &relay_ws_client,
            &mut rx,
        )
        .await;

        assert!(subs.is_empty());

        watch_topic_key
    };

    let app_subscribe_public_key = &subscribe_topic_response_body.subscribe_key;
    let app_authentication_public_key = &subscribe_topic_response_body.authentication_key;
    let dapp_did_key = DecodedClientId(
        hex::decode(app_authentication_public_key)
            .unwrap()
            .as_slice()
            .try_into()
            .unwrap(),
    )
    .to_did_key();

    // Get subscribe topic for dapp
    let subscribe_topic = topic_from_key(hex::decode(app_subscribe_public_key).unwrap().as_slice());

    // ----------------------------------------------------
    // SUBSCRIBE WALLET CLIENT TO DAPP THROUGHT NOTIFY
    // ----------------------------------------------------

    // Prepare subscription auth for *wallet* client
    // https://github.com/WalletConnect/walletconnect-docs/blob/main/docs/specs/clients/notify/notify-authentication.md#notify-subscription
    let notification_type = Uuid::new_v4();
    let notification_types = HashSet::from([notification_type, Uuid::new_v4()]);
    let now = Utc::now();
    let subscription_auth = SubscriptionRequestAuth {
        shared_claims: SharedClaims {
            iat: now.timestamp() as u64,
            exp: add_ttl(now, NOTIFY_SUBSCRIBE_TTL).timestamp() as u64,
            iss: identity_did_key.clone(),
            act: NOTIFY_SUBSCRIBE_ACT.to_owned(),
            aud: dapp_did_key.clone(),
            mjv: "0".to_owned(),
        },
        ksu: vars.keys_server_url.to_string(),
        sub: did_pkh.clone(),
        scp: encode_scope(&notification_types),
        app: DidWeb::from_domain(app_domain.clone()),
    };

    // Encode the subscription auth
    let subscription_auth = encode_auth(&subscription_auth, &identity_signing_key);

    let sub_auth = json!({ "subscriptionAuth": subscription_auth });
    let message = NotifyRequest::new(NOTIFY_SUBSCRIBE_METHOD, sub_auth);

    let subscription_secret = StaticSecret::random_from_rng(OsRng);
    let subscription_public = PublicKey::from(&subscription_secret);
    let response_topic_key = derive_key(
        &x25519_dalek::PublicKey::from(decode_key(app_subscribe_public_key).unwrap()),
        &subscription_secret,
    )
    .unwrap();

    let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(&response_topic_key));

    let envelope: Envelope<EnvelopeType1> = Envelope::<EnvelopeType1>::new(
        &response_topic_key,
        serde_json::to_value(message).unwrap(),
        *subscription_public.as_bytes(),
    )
    .unwrap();
    let message = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    // Get response topic for wallet client and notify communication
    let response_topic = topic_from_key(&response_topic_key);
    println!("subscription response_topic: {response_topic}");

    subscribe_relay_topic(&relay_ws_client, &response_topic, None)
        .await
        .unwrap();

    // Send subscription request to notify
    relay_ws_client
        .publish(
            subscribe_topic,
            message,
            NOTIFY_SUBSCRIBE_TAG,
            NOTIFY_SUBSCRIBE_TTL,
            false,
        )
        .await
        .unwrap();

    let resp = rx.recv().await.unwrap();
    let RelayClientEvent::Message(msg) = resp else {
        panic!("Expected message, got {:?}", resp);
    };
    let msg = if msg.tag == NOTIFY_SUBSCRIBE_RESPONSE_TAG {
        assert_eq!(msg.tag, NOTIFY_SUBSCRIBE_RESPONSE_TAG);
        msg
    } else {
        println!(
            "got additional message with unexpected tag {} msg.id {} and message_id {}",
            msg.tag,
            msg.message_id,
            get_message_id(&msg.message),
        );
        let Envelope::<EnvelopeType0> { sealbox, iv, .. } = Envelope::<EnvelopeType0>::from_bytes(
            base64::engine::general_purpose::STANDARD
                .decode(msg.message.as_bytes())
                .unwrap(),
        )
        .unwrap();
        let decrypted_response = ChaCha20Poly1305::new(GenericArray::from_slice(&watch_topic_key))
            .decrypt(&iv.into(), chacha20poly1305::aead::Payload::from(&*sealbox))
            .unwrap();
        let response: NotifyResponse<serde_json::Value> =
            serde_json::from_slice(&decrypted_response).unwrap();
        println!(
            "warn: got additional message with unexpected tag {} msg.id {} and message_id {} RPC ID {}",
            msg.tag,
            msg.message_id,
            get_message_id(&msg.message),
            response.id,
        );

        let resp = rx.recv().await.unwrap();
        let RelayClientEvent::Message(msg) = resp else {
            panic!("Expected message, got {:?}", resp);
        };
        assert_eq!(msg.tag, NOTIFY_SUBSCRIBE_RESPONSE_TAG);
        msg
    };

    let Envelope::<EnvelopeType0> { sealbox, iv, .. } = Envelope::<EnvelopeType0>::from_bytes(
        base64::engine::general_purpose::STANDARD
            .decode(msg.message.as_bytes())
            .unwrap(),
    )
    .unwrap();

    let decrypted_response = cipher
        .decrypt(&iv.into(), chacha20poly1305::aead::Payload::from(&*sealbox))
        .unwrap();

    let response: NotifyResponse<serde_json::Value> =
        serde_json::from_slice(&decrypted_response).unwrap();

    let response_auth = response
        .result
        .get("responseAuth") // TODO use structure
        .unwrap()
        .as_str()
        .unwrap();
    let subscribe_response_auth = from_jwt::<SubscriptionResponseAuth>(response_auth).unwrap();
    assert_eq!(
        subscribe_response_auth.shared_claims.act,
        NOTIFY_SUBSCRIBE_RESPONSE_ACT
    );

    let notify_key = {
        let resp = rx.recv().await.unwrap();

        let RelayClientEvent::Message(msg) = resp else {
            panic!("Expected message, got {:?}", resp);
        };
        assert_eq!(msg.tag, NOTIFY_SUBSCRIPTIONS_CHANGED_TAG);

        let Envelope::<EnvelopeType0> { sealbox, iv, .. } = Envelope::<EnvelopeType0>::from_bytes(
            base64::engine::general_purpose::STANDARD
                .decode(msg.message.as_bytes())
                .unwrap(),
        )
        .unwrap();

        let decrypted_response = ChaCha20Poly1305::new(GenericArray::from_slice(&watch_topic_key))
            .decrypt(&iv.into(), chacha20poly1305::aead::Payload::from(&*sealbox))
            .unwrap();

        let response: NotifyRequest<serde_json::Value> =
            serde_json::from_slice(&decrypted_response).unwrap();

        let response_auth = response
            .params
            .get("subscriptionsChangedAuth") // TODO use structure
            .unwrap()
            .as_str()
            .unwrap();
        let auth = from_jwt::<WatchSubscriptionsChangedRequestAuth>(response_auth).unwrap();
        assert_eq!(auth.shared_claims.act, NOTIFY_SUBSCRIPTIONS_CHANGED_ACT);
        assert_eq!(auth.sbs.len(), 1);
        let sub = &auth.sbs[0];
        assert_eq!(sub.scope, notification_types);
        assert_eq!(sub.account, account);
        assert_eq!(sub.app_domain, app_domain);
        assert_eq!(&sub.app_authentication_key, &dapp_did_key);
        assert_eq!(
            DecodedClientId::try_from_did_key(&sub.app_authentication_key)
                .unwrap()
                .0,
            decode_key(app_authentication_public_key).unwrap()
        );
        assert_eq!(sub.scope, notification_types);
        decode_key(&sub.sym_key).unwrap()
    };

    let notify_topic = topic_from_key(&notify_key);

    subscribe_relay_topic(&relay_ws_client, &notify_topic, None)
        .await
        .unwrap();

    let msg_4050 = rx.recv().await.unwrap();
    let RelayClientEvent::Message(msg) = msg_4050 else {
        panic!("Expected message, got {:?}", msg_4050);
    };
    assert_eq!(msg.tag, NOTIFY_NOOP_TAG);

    let notification = Notification {
        r#type: notification_type,
        title: "title".to_owned(),
        body: "body".to_owned(),
        icon: Some("icon".to_owned()),
        url: Some("url".to_owned()),
    };

    let notify_body = NotifyBody {
        notification_id: None,
        notification: notification.clone(),
        accounts: vec![account],
    };

    let _res = reqwest::Client::new()
        .post(format!(
            "{}/{}/notify",
            &vars.notify_url, &vars.notify_project_id
        ))
        .bearer_auth(&vars.notify_project_secret)
        .json(&notify_body)
        .send()
        .await
        .unwrap();

    let resp = rx.recv().await.unwrap();
    let RelayClientEvent::Message(msg) = resp else {
        panic!("Expected message, got {:?}", resp);
    };
    assert_eq!(msg.tag, NOTIFY_MESSAGE_TAG);

    let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(&notify_key));

    let Envelope::<EnvelopeType0> { iv, sealbox, .. } = Envelope::<EnvelopeType0>::from_bytes(
        base64::engine::general_purpose::STANDARD
            .decode(msg.message.as_bytes())
            .unwrap(),
    )
    .unwrap();

    // TODO: add proper type for that val
    let decrypted_notification: NotifyRequest<NotifyPayload> = serde_json::from_slice(
        &cipher
            .decrypt(&iv.into(), chacha20poly1305::aead::Payload::from(&*sealbox))
            .unwrap(),
    )
    .unwrap();

    // let received_notification = decrypted_notification.params;
    let claims = verify_jwt(
        &decrypted_notification.params.message_auth,
        &VerifyingKey::from_bytes(&decode_key(app_authentication_public_key).unwrap()).unwrap(),
    )
    .unwrap();

    // https://github.com/WalletConnect/walletconnect-docs/blob/main/docs/specs/clients/notify/notify-authentication.md#notify-message
    // TODO: verify issuer
    assert_eq!(claims.msg.r#type, notification.r#type);
    assert_eq!(claims.msg.title, notification.title);
    assert_eq!(claims.msg.body, notification.body);
    assert_eq!(claims.msg.icon, "icon");
    assert_eq!(claims.msg.url, "url");
    assert_eq!(claims.sub, did_pkh);
    assert!(claims.iat < chrono::Utc::now().timestamp() + JWT_LEEWAY); // TODO remove leeway
    assert!(claims.exp > chrono::Utc::now().timestamp() - JWT_LEEWAY); // TODO remove leeway
    assert_eq!(claims.app.as_ref(), app_domain);
    assert_eq!(claims.sub, did_pkh);
    assert_eq!(claims.act, NOTIFY_MESSAGE_ACT);

    // TODO Notify receipt?
    // https://github.com/WalletConnect/walletconnect-docs/blob/main/docs/specs/clients/notify/notify-authentication.md#notify-receipt

    // Update subscription

    // Prepare update auth for *wallet* client
    // https://github.com/WalletConnect/walletconnect-docs/blob/main/docs/specs/clients/notify/notify-authentication.md#notify-updatelet notification_type = Uuid::new_v4();
    let notification_type = Uuid::new_v4();
    let notification_types = HashSet::from([notification_type, Uuid::new_v4(), Uuid::new_v4()]);
    let now = Utc::now();
    let update_auth = SubscriptionUpdateRequestAuth {
        shared_claims: SharedClaims {
            iat: now.timestamp() as u64,
            exp: add_ttl(now, NOTIFY_UPDATE_TTL).timestamp() as u64,
            iss: identity_did_key.clone(),
            act: NOTIFY_UPDATE_ACT.to_owned(),
            aud: dapp_did_key.clone(),
            mjv: "0".to_owned(),
        },
        ksu: vars.keys_server_url.to_string(),
        sub: did_pkh.clone(),
        scp: encode_scope(&notification_types),
        app: DidWeb::from_domain(app_domain.clone()),
    };

    // Encode the subscription auth
    let update_auth = encode_auth(&update_auth, &identity_signing_key);

    let sub_auth = json!({ "updateAuth": update_auth });

    let delete_message = NotifyRequest::new(NOTIFY_UPDATE_METHOD, sub_auth);

    let envelope = Envelope::<EnvelopeType0>::new(&notify_key, delete_message).unwrap();

    let encoded_message = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    relay_ws_client
        .publish(
            notify_topic.clone(),
            encoded_message,
            NOTIFY_UPDATE_TAG,
            NOTIFY_UPDATE_TTL,
            false,
        )
        .await
        .unwrap();

    // Check for update response
    let resp = rx.recv().await.unwrap();

    let RelayClientEvent::Message(msg) = resp else {
        panic!("Expected message, got {:?}", resp);
    };
    assert_eq!(msg.tag, NOTIFY_UPDATE_RESPONSE_TAG);

    let Envelope::<EnvelopeType0> { sealbox, iv, .. } = Envelope::<EnvelopeType0>::from_bytes(
        base64::engine::general_purpose::STANDARD
            .decode(msg.message.as_bytes())
            .unwrap(),
    )
    .unwrap();

    let decrypted_response = cipher
        .decrypt(&iv.into(), chacha20poly1305::aead::Payload::from(&*sealbox))
        .unwrap();

    let response: NotifyResponse<serde_json::Value> =
        serde_json::from_slice(&decrypted_response).unwrap();

    let response_auth = response
        .result
        .get("responseAuth") // TODO use structure
        .unwrap()
        .as_str()
        .unwrap();
    let claims = from_jwt::<SubscriptionUpdateResponseAuth>(response_auth).unwrap();
    // https://github.com/WalletConnect/walletconnect-docs/blob/main/docs/specs/clients/notify/notify-authentication.md#notify-update-response
    // TODO verify issuer
    assert_eq!(claims.sub, did_pkh);
    assert!((claims.shared_claims.iat as i64) < chrono::Utc::now().timestamp() + JWT_LEEWAY); // TODO remove leeway
    assert!((claims.shared_claims.exp as i64) > chrono::Utc::now().timestamp() - JWT_LEEWAY); // TODO remove leeway
    assert_eq!(claims.app, DidWeb::from_domain(app_domain.clone()));
    assert_eq!(claims.shared_claims.aud, identity_did_key);
    assert_eq!(claims.shared_claims.act, NOTIFY_UPDATE_RESPONSE_ACT);

    {
        let resp = rx.recv().await.unwrap();

        let RelayClientEvent::Message(msg) = resp else {
            panic!("Expected message, got {:?}", resp);
        };
        assert_eq!(msg.tag, NOTIFY_SUBSCRIPTIONS_CHANGED_TAG);

        let Envelope::<EnvelopeType0> { sealbox, iv, .. } = Envelope::<EnvelopeType0>::from_bytes(
            base64::engine::general_purpose::STANDARD
                .decode(msg.message.as_bytes())
                .unwrap(),
        )
        .unwrap();

        let decrypted_response = ChaCha20Poly1305::new(GenericArray::from_slice(&watch_topic_key))
            .decrypt(&iv.into(), chacha20poly1305::aead::Payload::from(&*sealbox))
            .unwrap();

        let response: NotifyRequest<serde_json::Value> =
            serde_json::from_slice(&decrypted_response).unwrap();

        let response_auth = response
            .params
            .get("subscriptionsChangedAuth") // TODO use structure
            .unwrap()
            .as_str()
            .unwrap();
        let auth = from_jwt::<WatchSubscriptionsChangedRequestAuth>(response_auth).unwrap();
        assert_eq!(auth.shared_claims.act, NOTIFY_SUBSCRIPTIONS_CHANGED_ACT);
        assert_eq!(auth.sbs.len(), 1);
        let subs = &auth.sbs[0];
        assert_eq!(subs.scope, notification_types);
    }

    // Prepare deletion auth for *wallet* client
    // https://github.com/WalletConnect/walletconnect-docs/blob/main/docs/specs/clients/notify/notify-authentication.md#notify-delete
    let now = Utc::now();
    let delete_auth = SubscriptionDeleteRequestAuth {
        shared_claims: SharedClaims {
            iat: now.timestamp() as u64,
            exp: add_ttl(now, NOTIFY_DELETE_TTL).timestamp() as u64,
            iss: identity_did_key.clone(),
            aud: dapp_did_key.clone(),
            act: NOTIFY_DELETE_ACT.to_owned(),
            mjv: "0".to_owned(),
        },
        ksu: vars.keys_server_url.to_string(),
        sub: did_pkh.clone(),
        app: DidWeb::from_domain(app_domain.clone()),
    };

    // Encode the subscription auth
    let delete_auth = encode_auth(&delete_auth, &identity_signing_key);

    let sub_auth = json!({ "deleteAuth": delete_auth });

    let delete_message = NotifyRequest::new(NOTIFY_DELETE_METHOD, sub_auth);

    let envelope = Envelope::<EnvelopeType0>::new(&notify_key, delete_message).unwrap();

    let encoded_message = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    relay_ws_client
        .publish(
            notify_topic,
            encoded_message,
            NOTIFY_DELETE_TAG,
            NOTIFY_DELETE_TTL,
            false,
        )
        .await
        .unwrap();

    // Check for delete response
    let resp = rx.recv().await.unwrap();

    let RelayClientEvent::Message(msg) = resp else {
        panic!("Expected message, got {:?}", resp);
    };
    assert_eq!(msg.tag, NOTIFY_DELETE_RESPONSE_TAG);

    let Envelope::<EnvelopeType0> { sealbox, iv, .. } = Envelope::<EnvelopeType0>::from_bytes(
        base64::engine::general_purpose::STANDARD
            .decode(msg.message.as_bytes())
            .unwrap(),
    )
    .unwrap();

    let decrypted_response = cipher
        .decrypt(&iv.into(), chacha20poly1305::aead::Payload::from(&*sealbox))
        .unwrap();

    let response: NotifyResponse<serde_json::Value> =
        serde_json::from_slice(&decrypted_response).unwrap();

    let response_auth = response
        .result
        .get("responseAuth") // TODO use structure
        .unwrap()
        .as_str()
        .unwrap();
    let claims = from_jwt::<SubscriptionDeleteResponseAuth>(response_auth).unwrap();
    // https://github.com/WalletConnect/walletconnect-docs/blob/main/docs/specs/clients/notify/notify-authentication.md#notify-delete-response
    // TODO verify issuer
    assert_eq!(claims.sub, did_pkh);
    assert!((claims.shared_claims.iat as i64) < chrono::Utc::now().timestamp() + JWT_LEEWAY); // TODO remove leeway
    assert!((claims.shared_claims.exp as i64) > chrono::Utc::now().timestamp() - JWT_LEEWAY); // TODO remove leeway
    assert_eq!(claims.app, DidWeb::from_domain(app_domain));
    assert_eq!(claims.shared_claims.aud, identity_did_key);
    assert_eq!(claims.shared_claims.act, NOTIFY_DELETE_RESPONSE_ACT);

    {
        let resp = rx.recv().await.unwrap();

        let RelayClientEvent::Message(msg) = resp else {
            panic!("Expected message, got {:?}", resp);
        };
        assert_eq!(msg.tag, NOTIFY_SUBSCRIPTIONS_CHANGED_TAG);

        let Envelope::<EnvelopeType0> { sealbox, iv, .. } = Envelope::<EnvelopeType0>::from_bytes(
            base64::engine::general_purpose::STANDARD
                .decode(msg.message.as_bytes())
                .unwrap(),
        )
        .unwrap();

        let decrypted_response = ChaCha20Poly1305::new(GenericArray::from_slice(&watch_topic_key))
            .decrypt(&iv.into(), chacha20poly1305::aead::Payload::from(&*sealbox))
            .unwrap();

        let response: NotifyRequest<serde_json::Value> =
            serde_json::from_slice(&decrypted_response).unwrap();

        let response_auth = response
            .params
            .get("subscriptionsChangedAuth") // TODO use structure
            .unwrap()
            .as_str()
            .unwrap();
        let auth = from_jwt::<WatchSubscriptionsChangedRequestAuth>(response_auth).unwrap();
        assert_eq!(auth.shared_claims.act, NOTIFY_SUBSCRIPTIONS_CHANGED_ACT);
        assert!(auth.sbs.is_empty());
    }

    let resp = reqwest::Client::new()
        .post(format!(
            "{}/{}/notify",
            &vars.notify_url, &vars.notify_project_id
        ))
        .bearer_auth(vars.notify_project_secret)
        .json(&notify_body)
        .send()
        .await
        .unwrap();

    let resp = resp
        .json::<notify_server::services::public_http_server::handlers::notify_v0::ResponseBody>()
        .await
        .unwrap();

    assert_eq!(resp.not_found.len(), 1);

    let unregister_auth = UnregisterIdentityRequestAuth {
        shared_claims: SharedClaims {
            iat: Utc::now().timestamp() as u64,
            exp: Utc::now().timestamp() as u64 + 3600,
            iss: identity_did_key.clone(),
            aud: vars.keys_server_url.to_string(),
            act: "unregister_identity".to_owned(),
            mjv: "0".to_owned(),
        },
        pkh: did_pkh,
    };
    let unregister_auth = encode_auth(&unregister_auth, &identity_signing_key);
    reqwest::Client::new()
        .delete(vars.keys_server_url.join("/identity").unwrap())
        .body(serde_json::to_string(&json!({"idAuth": unregister_auth})).unwrap())
        .send()
        .await
        .unwrap();

    if let Ok(resp) = tokio::time::timeout(RELAY_MESSAGE_DELIVERY_TIMEOUT, rx.recv()).await {
        let resp = resp.unwrap();
        let RelayClientEvent::Message(msg) = resp else {
            panic!("Expected message, got {:?}", resp);
        };
        println!(
            "warn: received extra left-over message with tag {}",
            msg.tag
        );
    }
}

#[tokio::test]
async fn notify_this_domain() {
    run_test(STATEMENT_THIS_DOMAIN.to_owned(), false).await
}
