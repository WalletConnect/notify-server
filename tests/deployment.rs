use {
    crate::utils::{create_client, verify_jwt, JWT_LEEWAY},
    base64::Engine,
    chacha20poly1305::{
        aead::{generic_array::GenericArray, Aead, OsRng},
        ChaCha20Poly1305, KeyInit,
    },
    chrono::Utc,
    data_encoding::BASE64URL,
    ed25519_dalek::{Signer, SigningKey, VerifyingKey},
    hyper::StatusCode,
    lazy_static::lazy_static,
    notify_server::{
        auth::{
            add_ttl, from_jwt, GetSharedClaims, NotifyServerSubscription, SharedClaims,
            SubscriptionDeleteRequestAuth, SubscriptionDeleteResponseAuth, SubscriptionRequestAuth,
            SubscriptionResponseAuth, SubscriptionUpdateRequestAuth,
            SubscriptionUpdateResponseAuth, WatchSubscriptionsChangedRequestAuth,
            WatchSubscriptionsRequestAuth, WatchSubscriptionsResponseAuth, STATEMENT,
            STATEMENT_ALL_DOMAINS, STATEMENT_THIS_DOMAIN,
        },
        handlers::{notify_v0::NotifyBody, subscribe_topic::SubscribeTopicRequestData},
        jsonrpc::NotifyPayload,
        model::types::AccountId,
        spec::{
            NOTIFY_DELETE_METHOD, NOTIFY_DELETE_RESPONSE_TAG, NOTIFY_DELETE_TAG, NOTIFY_DELETE_TTL,
            NOTIFY_MESSAGE_TAG, NOTIFY_NOOP, NOTIFY_SUBSCRIBE_METHOD,
            NOTIFY_SUBSCRIBE_RESPONSE_TAG, NOTIFY_SUBSCRIBE_TAG, NOTIFY_SUBSCRIBE_TTL,
            NOTIFY_SUBSCRIPTIONS_CHANGED_TAG, NOTIFY_UPDATE_METHOD, NOTIFY_UPDATE_RESPONSE_TAG,
            NOTIFY_UPDATE_TAG, NOTIFY_UPDATE_TTL, NOTIFY_WATCH_SUBSCRIPTIONS_METHOD,
            NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_TAG, NOTIFY_WATCH_SUBSCRIPTIONS_TAG,
            NOTIFY_WATCH_SUBSCRIPTIONS_TTL,
        },
        types::{Envelope, EnvelopeType0, EnvelopeType1, Notification},
        websocket_service::{
            decode_key, derive_key, NotifyRequest, NotifyResponse, NotifyWatchSubscriptions,
        },
        wsclient::RelayClientEvent,
    },
    rand::{rngs::StdRng, SeedableRng},
    relay_rpc::{
        auth::{
            cacao::{self, signature::Eip191},
            ed25519_dalek::Keypair,
        },
        domain::DecodedClientId,
        jwt::{JwtHeader, JWT_HEADER_ALG, JWT_HEADER_TYP},
    },
    serde::Serialize,
    serde_json::json,
    sha2::Digest,
    sha3::Keccak256,
    std::collections::HashSet,
    tokio::sync::mpsc::UnboundedReceiver,
    url::Url,
    x25519_dalek::{PublicKey, StaticSecret},
};

mod utils;

lazy_static! {
    static ref KEYS_SERVER: Url = "https://keys.walletconnect.com".parse().unwrap();
}

fn urls(env: String) -> (String, String) {
    match env.as_str() {
        "PROD" => (
            "https://notify.walletconnect.com".to_owned(),
            "wss://relay.walletconnect.com".to_owned(),
        ),
        "STAGING" => (
            "https://staging.notify.walletconnect.com".to_owned(),
            "wss://staging.relay.walletconnect.com".to_owned(),
        ),
        "DEV" => (
            "https://dev.notify.walletconnect.com".to_owned(),
            "wss://staging.relay.walletconnect.com".to_owned(),
        ),
        "LOCAL" => (
            "http://127.0.0.1:3000".to_owned(),
            "wss://staging.relay.walletconnect.com".to_owned(),
        ),
        e => panic!("Invalid environment: {}", e),
    }
}

fn decode_authentication_public_key(authentication_public_key: &str) -> VerifyingKey {
    VerifyingKey::from_bytes(&decode_key(authentication_public_key).unwrap()).unwrap()
}

#[allow(clippy::too_many_arguments)]
async fn watch_subscriptions(
    app_domain: Option<&str>,
    notify_url: &str,
    identity_signing_key: &SigningKey,
    identity_did_key: &str,
    did_pkh: &str,
    wsclient: &relay_client::websocket::Client,
    rx: &mut UnboundedReceiver<RelayClientEvent>,
) -> (Vec<NotifyServerSubscription>, [u8; 32]) {
    let (key_agreement_key, authentication_key) = {
        let did_json_url = Url::parse(notify_url)
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
            act: "notify_watch_subscriptions".to_owned(),
            aud: format!("did:key:{}", &DecodedClientId(authentication_key)),
        },
        ksu: KEYS_SERVER.to_string(),
        sub: did_pkh.to_owned(),
        app: app_domain.map(|app_domain| format!("did:web:{app_domain}")),
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
    let response_topic = sha256::digest(&response_topic_key);
    println!("watch_subscriptions response_topic: {response_topic}");

    let envelope =
        Envelope::<EnvelopeType1>::new(&response_topic_key, message, *public.as_bytes()).unwrap();
    let message = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    let watch_subscriptions_topic = sha256::digest(&key_agreement_key);
    wsclient
        .publish(
            watch_subscriptions_topic.into(),
            message,
            NOTIFY_WATCH_SUBSCRIPTIONS_TAG,
            NOTIFY_WATCH_SUBSCRIPTIONS_TTL,
            false,
        )
        .await
        .unwrap();

    wsclient
        .subscribe(response_topic.clone().into())
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
        sha256::digest(msg.message.as_ref()),
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
        "notify_watch_subscriptions_response"
    );
    assert_eq!(
        auth.shared_claims.iss,
        format!("did:key:{}", DecodedClientId(authentication_key))
    );

    (auth.sbs, response_topic_key)
}

async fn run_test(statement: String, watch_subscriptions_all_domains: bool) {
    let env = std::env::var("ENVIRONMENT").unwrap_or("LOCAL".to_owned());
    let (notify_url, relay_url) = urls(env);
    let project_id =
        std::env::var("TEST_PROJECT_ID").expect("Tests requires TEST_PROJECT_ID to be set");
    let relay_project_id = std::env::var("TEST_RELAY_PROJECT_ID").unwrap_or(project_id.clone());

    let (identity_signing_key, identity_did_key) = {
        let keypair = Keypair::generate(&mut StdRng::from_entropy());
        let signing_key = SigningKey::from_bytes(keypair.secret_key().as_bytes());
        let client_id = DecodedClientId::from_key(&keypair.public_key());
        let client_did_key = format!("did:key:{client_id}");
        (signing_key, client_did_key)
    };

    let account_signing_key = k256::ecdsa::SigningKey::random(&mut OsRng);
    let address = &Keccak256::default()
        .chain_update(
            &account_signing_key
                .verifying_key()
                .to_encoded_point(false)
                .as_bytes()[1..],
        )
        .finalize()[12..];
    let account: AccountId = format!("eip155:1:0x{}", hex::encode(address)).into();
    let did_pkh = format!("did:pkh:{account}");

    let app_domain = "app.example.com";

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
                resources: Some(vec!["https://keys.walletconnect.com".to_owned()]),
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
            .post(KEYS_SERVER.join("/identity").unwrap())
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
    //     let (wsclient, mut rx) = create_client(&relay_url, &relay_project_id, &notify_url).await;

    //     let (subs, _) = watch_subscriptions(
    //         app_domain,
    //         &notify_url,
    //         &identity_signing_key,
    //         &identity_did_key,
    //         &did_pkh,
    //         &wsclient,
    //         &mut rx,
    //     )
    //     .await;

    //     assert!(subs.is_empty());
    // }

    let (wsclient, mut rx) = create_client(&relay_url, &relay_project_id, &notify_url).await;

    // ==== subscribe topic ====

    // TODO rename to "TEST_PROJECT_SECRET"
    let project_secret =
        std::env::var("NOTIFY_PROJECT_SECRET").expect("NOTIFY_PROJECT_SECRET not set");

    // Register project - generating subscribe topic
    let subscribe_topic_response = reqwest::Client::new()
        .post(format!("{}/{}/subscribe-topic", &notify_url, &project_id))
        .bearer_auth(&project_secret)
        .json(&SubscribeTopicRequestData {
            app_domain: app_domain.to_owned(),
        })
        .send()
        .await
        .unwrap();
    assert_eq!(subscribe_topic_response.status(), StatusCode::OK);
    let subscribe_topic_response_body: serde_json::Value =
        subscribe_topic_response.json().await.unwrap();

    let watch_topic_key = {
        let (subs, watch_topic_key) = watch_subscriptions(
            if watch_subscriptions_all_domains {
                None
            } else {
                Some(app_domain)
            },
            &notify_url,
            &identity_signing_key,
            &identity_did_key,
            &did_pkh,
            &wsclient,
            &mut rx,
        )
        .await;

        assert!(subs.is_empty());

        watch_topic_key
    };

    // Get app public key
    // TODO use struct
    let app_subscribe_public_key = subscribe_topic_response_body
        .get("subscribeKey")
        .unwrap()
        .as_str()
        .unwrap();

    // TODO use struct
    let app_authentication_public_key = subscribe_topic_response_body
        .get("authenticationKey")
        .unwrap()
        .as_str()
        .unwrap();
    let dapp_did_key = format!(
        "did:key:{}",
        DecodedClientId(
            hex::decode(app_authentication_public_key)
                .unwrap()
                .as_slice()
                .try_into()
                .unwrap()
        )
    );

    // Get subscribe topic for dapp
    let subscribe_topic = sha256::digest(hex::decode(app_subscribe_public_key).unwrap().as_slice());

    // ----------------------------------------------------
    // SUBSCRIBE WALLET CLIENT TO DAPP THROUGHT NOTIFY
    // ----------------------------------------------------

    // Prepare subscription auth for *wallet* client
    // https://github.com/WalletConnect/walletconnect-docs/blob/main/docs/specs/clients/notify/notify-authentication.md#notify-subscription
    let now = Utc::now();
    let subscription_auth = SubscriptionRequestAuth {
        shared_claims: SharedClaims {
            iat: now.timestamp() as u64,
            exp: add_ttl(now, NOTIFY_SUBSCRIBE_TTL).timestamp() as u64,
            iss: identity_did_key.clone(),
            act: "notify_subscription".to_owned(),
            aud: dapp_did_key.clone(),
        },
        ksu: KEYS_SERVER.to_string(),
        sub: did_pkh.clone(),
        scp: "test test1".to_owned(),
        app: format!("did:web:{app_domain}"),
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
        message,
        *subscription_public.as_bytes(),
    )
    .unwrap();
    let message = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    // Get response topic for wallet client and notify communication
    let response_topic = sha256::digest(&response_topic_key);
    println!("subscription response_topic: {response_topic}");

    // Subscribe to the topic and listen for response
    wsclient
        .subscribe(response_topic.clone().into())
        .await
        .unwrap();

    // Send subscription request to notify
    wsclient
        .publish(
            subscribe_topic.into(),
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
            sha256::digest(msg.message.as_ref()),
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
            sha256::digest(msg.message.as_ref()),
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
        "notify_subscription_response"
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
        assert_eq!(auth.shared_claims.act, "notify_subscriptions_changed");
        assert_eq!(auth.sbs.len(), 1);
        let sub = &auth.sbs[0];
        assert_eq!(
            sub.scope,
            HashSet::from(["test".to_owned(), "test1".to_owned()])
        );
        assert_eq!(sub.account, account);
        assert_eq!(sub.app_domain, app_domain);
        assert_eq!(&sub.app_authentication_key, &dapp_did_key);
        assert_eq!(
            DecodedClientId::try_from_did_key(&sub.app_authentication_key)
                .unwrap()
                .0,
            decode_key(app_authentication_public_key).unwrap()
        );
        assert_eq!(
            sub.scope,
            HashSet::from(["test".to_owned(), "test1".to_owned()]),
        );
        decode_key(&sub.sym_key).unwrap()
    };

    let notify_topic = sha256::digest(&notify_key);

    wsclient
        .subscribe(notify_topic.clone().into())
        .await
        .unwrap();

    let msg_4050 = rx.recv().await.unwrap();
    let RelayClientEvent::Message(msg) = msg_4050 else {
        panic!("Expected message, got {:?}", msg_4050);
    };
    assert_eq!(msg.tag, NOTIFY_NOOP);

    let notification = Notification {
        title: "string".to_owned(),
        body: "string".to_owned(),
        icon: "string".to_owned(),
        url: "string".to_owned(),
        r#type: "test".to_owned(),
    };

    let notify_body = NotifyBody {
        notification: notification.clone(),
        accounts: vec![account],
    };

    // wait for notify server to register the user
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    let _res = reqwest::Client::new()
        .post(format!("{}/{}/notify", &notify_url, &project_id))
        .bearer_auth(&project_secret)
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
        &decode_authentication_public_key(app_authentication_public_key),
    )
    .unwrap();

    // https://github.com/WalletConnect/walletconnect-docs/blob/main/docs/specs/clients/notify/notify-authentication.md#notify-message
    // TODO: verify issuer
    assert_eq!(claims.msg.as_ref(), &notification);
    assert_eq!(claims.sub, did_pkh);
    assert!(claims.iat < chrono::Utc::now().timestamp() + JWT_LEEWAY); // TODO remove leeway
    assert!(claims.exp > chrono::Utc::now().timestamp() - JWT_LEEWAY); // TODO remove leeway
    assert_eq!(claims.app.as_ref(), app_domain);
    assert_eq!(claims.sub, did_pkh);
    assert_eq!(claims.act, "notify_message");

    // TODO Notify receipt?
    // https://github.com/WalletConnect/walletconnect-docs/blob/main/docs/specs/clients/notify/notify-authentication.md#notify-receipt

    // Update subscription

    // Prepare update auth for *wallet* client
    // https://github.com/WalletConnect/walletconnect-docs/blob/main/docs/specs/clients/notify/notify-authentication.md#notify-update
    let now = Utc::now();
    let update_auth = SubscriptionUpdateRequestAuth {
        shared_claims: SharedClaims {
            iat: now.timestamp() as u64,
            exp: add_ttl(now, NOTIFY_UPDATE_TTL).timestamp() as u64,
            iss: identity_did_key.clone(),
            act: "notify_update".to_owned(),
            aud: dapp_did_key.clone(),
        },
        ksu: KEYS_SERVER.to_string(),
        sub: did_pkh.clone(),
        scp: "test test2 test3".to_owned(),
        app: format!("did:web:{app_domain}"),
    };

    // Encode the subscription auth
    let update_auth = encode_auth(&update_auth, &identity_signing_key);

    let sub_auth = json!({ "updateAuth": update_auth });

    let delete_message = NotifyRequest::new(NOTIFY_UPDATE_METHOD, sub_auth);

    let envelope = Envelope::<EnvelopeType0>::new(&notify_key, delete_message).unwrap();

    let encoded_message = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    wsclient
        .publish(
            notify_topic.clone().into(),
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
    assert_eq!(claims.app, format!("did:web:{app_domain}"));
    assert_eq!(claims.shared_claims.aud, identity_did_key);
    assert_eq!(claims.shared_claims.act, "notify_update_response");

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
        assert_eq!(auth.shared_claims.act, "notify_subscriptions_changed");
        assert_eq!(auth.sbs.len(), 1);
        let subs = &auth.sbs[0];
        assert_eq!(
            subs.scope,
            HashSet::from(["test".to_owned(), "test2".to_owned(), "test3".to_owned()])
        );
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
            act: "notify_delete".to_owned(),
        },
        ksu: KEYS_SERVER.to_string(),
        sub: did_pkh.clone(),
        app: format!("did:web:{app_domain}"),
    };

    // Encode the subscription auth
    let delete_auth = encode_auth(&delete_auth, &identity_signing_key);
    let _delete_auth_hash = sha256::digest(&*delete_auth.clone());

    let sub_auth = json!({ "deleteAuth": delete_auth });

    let delete_message = NotifyRequest::new(NOTIFY_DELETE_METHOD, sub_auth);

    let envelope = Envelope::<EnvelopeType0>::new(&notify_key, delete_message).unwrap();

    let encoded_message = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    wsclient
        .publish(
            notify_topic.into(),
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
    assert_eq!(claims.app, format!("did:web:{app_domain}"));
    assert_eq!(claims.shared_claims.aud, identity_did_key);
    assert_eq!(claims.shared_claims.act, "notify_delete_response");

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
        assert_eq!(auth.shared_claims.act, "notify_subscriptions_changed");
        assert!(auth.sbs.is_empty());
    }

    // wait for notify server to unregister the user
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/{}/notify", &notify_url, &project_id))
        .bearer_auth(project_secret)
        .json(&notify_body)
        .send()
        .await
        .unwrap();

    let resp = resp
        .json::<notify_server::handlers::notify_v0::Response>()
        .await
        .unwrap();

    assert_eq!(resp.not_found.len(), 1);

    let unregister_auth = UnregisterIdentityRequestAuth {
        shared_claims: SharedClaims {
            iat: Utc::now().timestamp() as u64,
            exp: Utc::now().timestamp() as u64 + 3600,
            iss: identity_did_key.clone(),
            aud: KEYS_SERVER.to_string(),
            act: "unregister_identity".to_owned(),
        },
        pkh: did_pkh,
    };
    let unregister_auth = encode_auth(&unregister_auth, &identity_signing_key);
    reqwest::Client::new()
        .delete(KEYS_SERVER.join("/identity").unwrap())
        .body(serde_json::to_string(&json!({"idAuth": unregister_auth})).unwrap())
        .send()
        .await
        .unwrap();

    if let Ok(resp) = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv()).await {
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

// TODO make into storage test
#[tokio::test]
async fn notify_all_domains() {
    run_test(STATEMENT_ALL_DOMAINS.to_owned(), true).await
}

#[tokio::test]
async fn notify_this_domain() {
    run_test(STATEMENT_THIS_DOMAIN.to_owned(), false).await
}

#[tokio::test]
async fn old_siwe_compatible() {
    run_test(STATEMENT.to_owned(), false).await
}

#[tokio::test]
async fn old_old_siwe_compatible() {
    run_test(
        "I further authorize this DAPP to send and receive messages on my behalf for \
    this domain using my WalletConnect identity."
            .to_owned(),
        false,
    )
    .await
}

pub fn encode_auth<T: Serialize>(auth: &T, signing_key: &SigningKey) -> String {
    let data = JwtHeader {
        typ: JWT_HEADER_TYP,
        alg: JWT_HEADER_ALG,
    };
    let header = serde_json::to_string(&data).unwrap();
    let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(header);

    let claims = {
        let json = serde_json::to_string(auth).unwrap();
        base64::engine::general_purpose::STANDARD_NO_PAD.encode(json)
    };

    let message = format!("{header}.{claims}");

    let signature = signing_key.sign(message.as_bytes());
    let signature = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature.to_bytes());

    format!("{message}.{signature}")
}

#[derive(Debug, Clone, Serialize)]
pub struct UnregisterIdentityRequestAuth {
    #[serde(flatten)]
    pub shared_claims: SharedClaims,
    /// corresponding blockchain account (did:pkh)
    pub pkh: String,
}

impl GetSharedClaims for UnregisterIdentityRequestAuth {
    fn get_shared_claims(&self) -> &SharedClaims {
        &self.shared_claims
    }
}
