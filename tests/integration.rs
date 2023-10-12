use {
    base64::Engine,
    chacha20poly1305::{
        aead::{generic_array::GenericArray, Aead, OsRng},
        ChaCha20Poly1305,
        KeyInit,
    },
    chrono::Utc,
    data_encoding::BASE64URL,
    ed25519_dalek::{Signer, SigningKey},
    hyper::StatusCode,
    lazy_static::lazy_static,
    notify_server::{
        auth::{
            add_ttl,
            from_jwt,
            AuthError,
            GetSharedClaims,
            NotifyServerSubscription,
            SharedClaims,
            SubscriptionDeleteRequestAuth,
            SubscriptionDeleteResponseAuth,
            SubscriptionRequestAuth,
            SubscriptionResponseAuth,
            SubscriptionUpdateRequestAuth,
            SubscriptionUpdateResponseAuth,
            WatchSubscriptionsChangedRequestAuth,
            WatchSubscriptionsRequestAuth,
            WatchSubscriptionsResponseAuth,
            STATEMENT,
            STATEMENT_ALL_DOMAINS,
        },
        handlers::{notify::JwtMessage, subscribe_topic::SubscribeTopicRequestData},
        jsonrpc::NotifyPayload,
        spec::{
            NOTIFY_DELETE_METHOD,
            NOTIFY_DELETE_RESPONSE_TAG,
            NOTIFY_DELETE_TAG,
            NOTIFY_DELETE_TTL,
            NOTIFY_MESSAGE_TAG,
            NOTIFY_SUBSCRIBE_METHOD,
            NOTIFY_SUBSCRIBE_RESPONSE_TAG,
            NOTIFY_SUBSCRIBE_TAG,
            NOTIFY_SUBSCRIBE_TTL,
            NOTIFY_SUBSCRIPTIONS_CHANGED_TAG,
            NOTIFY_UPDATE_METHOD,
            NOTIFY_UPDATE_RESPONSE_TAG,
            NOTIFY_UPDATE_TAG,
            NOTIFY_UPDATE_TTL,
            NOTIFY_WATCH_SUBSCRIPTIONS_METHOD,
            NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_TAG,
            NOTIFY_WATCH_SUBSCRIPTIONS_TAG,
            NOTIFY_WATCH_SUBSCRIPTIONS_TTL,
        },
        types::{Envelope, EnvelopeType0, EnvelopeType1, Notification},
        websocket_service::{
            decode_key,
            derive_key,
            NotifyRequest,
            NotifyResponse,
            NotifyWatchSubscriptions,
        },
        wsclient::{self, RelayClientEvent},
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
    std::sync::Arc,
    tokio::sync::mpsc::UnboundedReceiver,
    url::Url,
    x25519_dalek::{PublicKey, StaticSecret},
};

const JWT_LEEWAY: i64 = 30;

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
        _ => panic!("Invalid environment"),
    }
}

#[tokio::test]
async fn notify_properly_sending_message() {
    let env = std::env::var("ENVIRONMENT").unwrap_or("LOCAL".to_owned());
    let (notify_url, relay_url) = urls(env);
    let project_id =
        std::env::var("TEST_PROJECT_ID").expect("Tests requires TEST_PROJECT_ID to be set");
    let relay_project_id = std::env::var("TEST_RELAY_PROJECT_ID").unwrap_or(project_id.clone());

    let keypair = Keypair::generate(&mut StdRng::from_entropy());
    let signing_key = SigningKey::from_bytes(keypair.secret_key().as_bytes());
    let client_id = DecodedClientId::from_key(&keypair.public_key());
    let client_did_key = format!("did:key:{client_id}");

    let account_signing_key = k256::ecdsa::SigningKey::random(&mut OsRng);
    let address = &Keccak256::default()
        .chain_update(
            &account_signing_key
                .verifying_key()
                .to_encoded_point(false)
                .as_bytes()[1..],
        )
        .finalize()[12..];
    let account = format!("eip155:1:0x{}", hex::encode(address));
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
                statement: Some(STATEMENT_ALL_DOMAINS.to_owned()),
                aud: client_did_key.clone(),
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

    async fn create_client(
        relay_url: &str,
        relay_project_id: &str,
        keypair: &Keypair,
        notify_url: &str,
    ) -> (
        StaticSecret,
        x25519_dalek::PublicKey,
        Arc<relay_client::websocket::Client>,
        UnboundedReceiver<RelayClientEvent>,
    ) {
        let secret = StaticSecret::random_from_rng(OsRng);
        let public = PublicKey::from(&secret);

        // Create a websocket client to communicate with relay
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let connection_handler = wsclient::RelayConnectionHandler::new("notify-client", tx);
        let wsclient = Arc::new(relay_client::websocket::Client::new(connection_handler));

        let opts =
            wsclient::create_connection_opts(relay_url, relay_project_id, keypair, notify_url)
                .unwrap();
        wsclient.connect(&opts).await.unwrap();

        // Eat up the "connected" message
        _ = rx.recv().await.unwrap();

        (secret, public, wsclient, rx)
    }

    #[allow(clippy::too_many_arguments)]
    async fn watch_subscriptions(
        app_domain: &str,
        notify_url: &str,
        signing_key: &SigningKey,
        client_did_key: &str,
        did_pkh: &str,
        secret: &StaticSecret,
        public: &x25519_dalek::PublicKey,
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
                iss: client_did_key.to_owned(),
                act: "notify_watch_subscriptions".to_owned(),
                aud: client_did_key.to_owned(), // TODO should be dapp key not client_id
            },
            ksu: KEYS_SERVER.to_string(),
            sub: did_pkh.to_owned(),
            app: Some(format!("did:web:{app_domain}")),
        };

        let message = NotifyRequest::new(
            NOTIFY_WATCH_SUBSCRIPTIONS_METHOD,
            NotifyWatchSubscriptions {
                watch_subscriptions_auth: encode_auth(&subscription_auth, signing_key),
            },
        );

        let response_topic_key =
            derive_key(&x25519_dalek::PublicKey::from(key_agreement_key), secret).unwrap();
        let response_topic = sha256::digest(&response_topic_key);

        let envelope =
            Envelope::<EnvelopeType1>::new(&response_topic_key, message, *public.as_bytes())
                .unwrap();
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

        let decrypted_response =
            ChaCha20Poly1305::new(GenericArray::from_slice(&response_topic_key))
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

    // ==== watchSubscriptions ====
    {
        let (secret, public, wsclient, mut rx) =
            create_client(&relay_url, &relay_project_id, &keypair, &notify_url).await;

        let (subs, _) = watch_subscriptions(
            app_domain,
            &notify_url,
            &signing_key,
            &client_did_key,
            &did_pkh,
            &secret,
            &public,
            &wsclient,
            &mut rx,
        )
        .await;

        assert!(subs.is_empty());
    }

    let (secret, public, wsclient, mut rx) =
        create_client(&relay_url, &relay_project_id, &keypair, &notify_url).await;

    // ==== subscribe topic ====

    // TODO rename to "TEST_PROJECT_SECRET"
    let project_secret =
        std::env::var("NOTIFY_PROJECT_SECRET").expect("NOTIFY_PROJECT_SECRET not set");

    // Register project - generating subscribe topic
    let subscribe_topic_response = reqwest::Client::new()
        .post(format!("{}/{}/subscribe-topic", &notify_url, &project_id))
        .bearer_auth(&project_secret)
        .json(&json!({ "appDomain": app_domain }))
        .send()
        .await
        .unwrap();
    assert_eq!(subscribe_topic_response.status(), StatusCode::OK);
    let subscribe_topic_response_body: serde_json::Value =
        subscribe_topic_response.json().await.unwrap();

    let watch_topic_key = {
        let (subs, watch_topic_key) = watch_subscriptions(
            app_domain,
            &notify_url,
            &signing_key,
            &client_did_key,
            &did_pkh,
            &secret,
            &public,
            &wsclient,
            &mut rx,
        )
        .await;

        assert!(subs.is_empty());

        watch_topic_key
    };

    // Get app public key
    // TODO use struct
    let dapp_pubkey = subscribe_topic_response_body
        .get("subscribeKey")
        .unwrap()
        .as_str()
        .unwrap();

    // TODO use struct
    let dapp_identity_pubkey = subscribe_topic_response_body
        .get("authenticationKey")
        .unwrap()
        .as_str()
        .unwrap();

    // Get subscribe topic for dapp
    let subscribe_topic = sha256::digest(hex::decode(dapp_pubkey).unwrap().as_slice());

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
            iss: format!("did:key:{client_id}"),
            act: "notify_subscription".to_owned(),
            aud: format!("did:key:{client_id}"), // TODO should be dapp key not client_id
        },
        ksu: KEYS_SERVER.to_string(),
        sub: did_pkh.clone(),
        scp: "test test1".to_owned(),
        app: format!("did:web:{app_domain}"),
    };

    // Encode the subscription auth
    let subscription_auth = encode_auth(&subscription_auth, &signing_key);

    let sub_auth = json!({ "subscriptionAuth": subscription_auth });
    let message = NotifyRequest::new(NOTIFY_SUBSCRIBE_METHOD, sub_auth);

    let response_topic_key = derive_key(
        &x25519_dalek::PublicKey::from(decode_key(dapp_pubkey).unwrap()),
        &secret,
    )
    .unwrap();

    let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(&response_topic_key));

    let envelope =
        Envelope::<EnvelopeType1>::new(&response_topic_key, message, *public.as_bytes()).unwrap();
    let message = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    // Get response topic for wallet client and notify communication
    let response_topic = sha256::digest(&response_topic_key);

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
    assert_eq!(msg.tag, NOTIFY_SUBSCRIBE_RESPONSE_TAG);

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
        // assert_eq!(
        //     sub.scope,
        //     HashSet::from(["test".to_owned(), "test1".to_owned()])
        // );
        assert_eq!(sub.account, account);
        assert_eq!(sub.app_domain, app_domain);
        // assert_eq!(
        //     sub.scope,
        //     HashSet::from(["test".to_owned(), "test1".to_owned()]),
        // );
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
    assert_eq!(msg.tag, 4050);

    let notification = Notification {
        title: "string".to_owned(),
        body: "string".to_owned(),
        icon: "string".to_owned(),
        url: "string".to_owned(),
        r#type: "test".to_owned(),
    };

    let notify_body = json!({
        "notification": notification,
        "accounts": [account]
    });

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
        dapp_identity_pubkey,
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
            iss: format!("did:key:{}", client_id),
            act: "notify_update".to_owned(),
            aud: format!("did:key:{}", client_id), // TODO should be dapp key not client_id
        },
        ksu: KEYS_SERVER.to_string(),
        sub: did_pkh.clone(),
        scp: "test test2 test3".to_owned(),
        app: format!("did:web:{app_domain}"),
    };

    // Encode the subscription auth
    let update_auth = encode_auth(&update_auth, &signing_key);

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
    assert_eq!(claims.shared_claims.aud, format!("did:key:{}", client_id));
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
        // let subs = &auth.sbs[0];
        // assert_eq!(
        //     subs.scope,
        //     HashSet::from(["test".to_owned(), "test2".to_owned(), "test3".to_owned()])
        // );
    }

    // Prepare deletion auth for *wallet* client
    // https://github.com/WalletConnect/walletconnect-docs/blob/main/docs/specs/clients/notify/notify-authentication.md#notify-delete
    let now = Utc::now();
    let delete_auth = SubscriptionDeleteRequestAuth {
        shared_claims: SharedClaims {
            iat: now.timestamp() as u64,
            exp: add_ttl(now, NOTIFY_DELETE_TTL).timestamp() as u64,
            iss: format!("did:key:{}", client_id),
            aud: format!("did:key:{}", client_id), // TODO should be dapp key not client_id
            act: "notify_delete".to_owned(),
        },
        ksu: KEYS_SERVER.to_string(),
        sub: did_pkh.clone(),
        app: format!("did:web:{app_domain}"),
    };

    // Encode the subscription auth
    let delete_auth = encode_auth(&delete_auth, &signing_key);
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
    assert_eq!(claims.shared_claims.aud, format!("did:key:{}", client_id));
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
        .json::<notify_server::handlers::notify::Response>()
        .await
        .unwrap();

    assert_eq!(resp.not_found.len(), 1);

    let unregister_auth = UnregisterIdentityRequestAuth {
        shared_claims: SharedClaims {
            iat: Utc::now().timestamp() as u64,
            exp: Utc::now().timestamp() as u64 + 3600,
            iss: client_did_key,
            aud: KEYS_SERVER.to_string(),
            act: "unregister_identity".to_owned(),
        },
        pkh: did_pkh,
    };
    let unregister_auth = encode_auth(&unregister_auth, &signing_key);
    reqwest::Client::new()
        .delete(KEYS_SERVER.join("/identity").unwrap())
        .body(serde_json::to_string(&json!({"idAuth": unregister_auth})).unwrap())
        .send()
        .await
        .unwrap();
}

#[tokio::test]
async fn old_siwe_compatible() {
    let env = std::env::var("ENVIRONMENT").unwrap_or("LOCAL".to_owned());
    let (notify_url, relay_url) = urls(env);
    let project_id =
        std::env::var("TEST_PROJECT_ID").expect("Tests requires TEST_PROJECT_ID to be set");
    let relay_project_id = std::env::var("TEST_RELAY_PROJECT_ID").unwrap_or(project_id.clone());

    let keypair = Keypair::generate(&mut StdRng::from_entropy());
    let signing_key = SigningKey::from_bytes(keypair.secret_key().as_bytes());
    let client_id = DecodedClientId::from_key(&keypair.public_key());
    let client_did_key = format!("did:key:{client_id}");

    let account_signing_key = k256::ecdsa::SigningKey::random(&mut OsRng);
    let address = &Keccak256::default()
        .chain_update(
            &account_signing_key
                .verifying_key()
                .to_encoded_point(false)
                .as_bytes()[1..],
        )
        .finalize()[12..];
    let account = format!("eip155:1:0x{}", hex::encode(address));
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
                statement: Some(STATEMENT.to_owned()),
                aud: client_did_key.clone(),
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

    async fn create_client(
        relay_url: &str,
        relay_project_id: &str,
        keypair: &Keypair,
        notify_url: &str,
    ) -> (
        StaticSecret,
        x25519_dalek::PublicKey,
        Arc<relay_client::websocket::Client>,
        UnboundedReceiver<RelayClientEvent>,
    ) {
        let secret = StaticSecret::random_from_rng(OsRng);
        let public = PublicKey::from(&secret);

        // Create a websocket client to communicate with relay
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let connection_handler = wsclient::RelayConnectionHandler::new("notify-client", tx);
        let wsclient = Arc::new(relay_client::websocket::Client::new(connection_handler));

        let opts =
            wsclient::create_connection_opts(relay_url, relay_project_id, keypair, notify_url)
                .unwrap();
        wsclient.connect(&opts).await.unwrap();

        // Eat up the "connected" message
        _ = rx.recv().await.unwrap();

        (secret, public, wsclient, rx)
    }

    #[allow(clippy::too_many_arguments)]
    async fn watch_subscriptions(
        app_domain: &str,
        notify_url: &str,
        signing_key: &SigningKey,
        client_did_key: &str,
        did_pkh: &str,
        secret: &StaticSecret,
        public: &x25519_dalek::PublicKey,
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
                iss: client_did_key.to_owned(),
                act: "notify_watch_subscriptions".to_owned(),
                aud: client_did_key.to_owned(), // TODO should be dapp key not client_id
            },
            ksu: KEYS_SERVER.to_string(),
            sub: did_pkh.to_owned(),
            app: Some(format!("did:web:{app_domain}")),
        };

        let message = NotifyRequest::new(
            NOTIFY_WATCH_SUBSCRIPTIONS_METHOD,
            NotifyWatchSubscriptions {
                watch_subscriptions_auth: encode_auth(&subscription_auth, signing_key),
            },
        );

        let response_topic_key =
            derive_key(&x25519_dalek::PublicKey::from(key_agreement_key), secret).unwrap();
        let response_topic = sha256::digest(&response_topic_key);

        let envelope =
            Envelope::<EnvelopeType1>::new(&response_topic_key, message, *public.as_bytes())
                .unwrap();
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

        let decrypted_response =
            ChaCha20Poly1305::new(GenericArray::from_slice(&response_topic_key))
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

    let (secret, public, wsclient, mut rx) =
        create_client(&relay_url, &relay_project_id, &keypair, &notify_url).await;

    // ==== subscribe topic ====

    // TODO rename to "TEST_PROJECT_SECRET"
    let project_secret =
        std::env::var("NOTIFY_PROJECT_SECRET").expect("NOTIFY_PROJECT_SECRET not set");

    // Register project - generating subscribe topic
    let subscribe_topic_response = reqwest::Client::new()
        .post(format!("{notify_url}/{project_id}/subscribe-topic"))
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
            app_domain,
            &notify_url,
            &signing_key,
            &client_did_key,
            &did_pkh,
            &secret,
            &public,
            &wsclient,
            &mut rx,
        )
        .await;

        assert!(subs.is_empty());

        watch_topic_key
    };

    // Get app public key
    // TODO use struct
    let dapp_pubkey = subscribe_topic_response_body
        .get("subscribeKey")
        .unwrap()
        .as_str()
        .unwrap();

    // TODO use struct
    let dapp_identity_pubkey = subscribe_topic_response_body
        .get("authenticationKey")
        .unwrap()
        .as_str()
        .unwrap();

    // Get subscribe topic for dapp
    let subscribe_topic = sha256::digest(hex::decode(dapp_pubkey).unwrap().as_slice());

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
            iss: format!("did:key:{client_id}"),
            act: "notify_subscription".to_owned(),
            aud: format!("did:key:{client_id}"), // TODO should be dapp key not client_id
        },
        ksu: KEYS_SERVER.to_string(),
        sub: did_pkh.clone(),
        scp: "test test1".to_owned(),
        app: format!("did:web:{app_domain}"),
    };

    // Encode the subscription auth
    let subscription_auth = encode_auth(&subscription_auth, &signing_key);

    let sub_auth = json!({ "subscriptionAuth": subscription_auth });
    let message = NotifyRequest::new(NOTIFY_SUBSCRIBE_METHOD, sub_auth);

    let response_topic_key = derive_key(
        &x25519_dalek::PublicKey::from(decode_key(dapp_pubkey).unwrap()),
        &secret,
    )
    .unwrap();

    let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(&response_topic_key));

    let envelope =
        Envelope::<EnvelopeType1>::new(&response_topic_key, message, *public.as_bytes()).unwrap();
    let message = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    // Get response topic for wallet client and notify communication
    let response_topic = sha256::digest(&response_topic_key);

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
    assert_eq!(msg.tag, NOTIFY_SUBSCRIBE_RESPONSE_TAG);

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
        // assert_eq!(
        //     sub.scope,
        //     HashSet::from(["test".to_owned(), "test1".to_owned()])
        // );
        assert_eq!(sub.account, account);
        assert_eq!(sub.app_domain, app_domain);
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
    assert_eq!(msg.tag, 4050);

    let notification = Notification {
        title: "string".to_owned(),
        body: "string".to_owned(),
        icon: "string".to_owned(),
        url: "string".to_owned(),
        r#type: "test".to_owned(),
    };

    let notify_body = json!({
        "notification": notification,
        "accounts": [account]
    });

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
        dapp_identity_pubkey,
    )
    .unwrap();

    // https://github.com/WalletConnect/walletconnect-docs/blob/main/docs/specs/clients/notify/notify-authentication.md#notify-message
    // TODO: verify issuer
    assert_eq!(*claims.msg, notification);
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
            iss: format!("did:key:{}", client_id),
            act: "notify_update".to_owned(),
            aud: format!("did:key:{}", client_id), // TODO should be dapp key not client_id
        },
        ksu: KEYS_SERVER.to_string(),
        sub: did_pkh.clone(),
        scp: "test test2 test3".to_owned(),
        app: format!("did:web:{app_domain}"),
    };

    // Encode the subscription auth
    let update_auth = encode_auth(&update_auth, &signing_key);

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
    assert_eq!(claims.shared_claims.aud, format!("did:key:{}", client_id));
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
        // let subs = &auth.sbs[0];
        // assert_eq!(
        //     subs.scope,
        //     HashSet::from(["test".to_owned(), "test2".to_owned(),
        // "test3".to_owned()]) );
    }

    // Prepare deletion auth for *wallet* client
    // https://github.com/WalletConnect/walletconnect-docs/blob/main/docs/specs/clients/notify/notify-authentication.md#notify-delete
    let now = Utc::now();
    let delete_auth = SubscriptionDeleteRequestAuth {
        shared_claims: SharedClaims {
            iat: now.timestamp() as u64,
            exp: add_ttl(now, NOTIFY_DELETE_TTL).timestamp() as u64,
            iss: format!("did:key:{}", client_id),
            aud: format!("did:key:{}", client_id), // TODO should be dapp key not client_id
            act: "notify_delete".to_owned(),
        },
        ksu: KEYS_SERVER.to_string(),
        sub: did_pkh.clone(),
        app: format!("did:web:{app_domain}"),
    };

    // Encode the subscription auth
    let delete_auth = encode_auth(&delete_auth, &signing_key);
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
    assert_eq!(claims.shared_claims.aud, format!("did:key:{}", client_id));
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
        .json::<notify_server::handlers::notify::Response>()
        .await
        .unwrap();

    assert_eq!(resp.not_found.len(), 1);

    let unregister_auth = UnregisterIdentityRequestAuth {
        shared_claims: SharedClaims {
            iat: Utc::now().timestamp() as u64,
            exp: Utc::now().timestamp() as u64 + 3600,
            iss: client_did_key,
            aud: KEYS_SERVER.to_string(),
            act: "unregister_identity".to_owned(),
        },
        pkh: did_pkh,
    };
    let unregister_auth = encode_auth(&unregister_auth, &signing_key);
    reqwest::Client::new()
        .delete(KEYS_SERVER.join("/identity").unwrap())
        .body(serde_json::to_string(&json!({"idAuth": unregister_auth})).unwrap())
        .send()
        .await
        .unwrap();
}

#[tokio::test]
async fn old_old_siwe_compatible() {
    let env = std::env::var("ENVIRONMENT").unwrap_or("LOCAL".to_owned());
    let (notify_url, relay_url) = urls(env);
    let project_id =
        std::env::var("TEST_PROJECT_ID").expect("Tests requires TEST_PROJECT_ID to be set");
    let relay_project_id = std::env::var("TEST_RELAY_PROJECT_ID").unwrap_or(project_id.clone());

    let keypair = Keypair::generate(&mut StdRng::from_entropy());
    let signing_key = SigningKey::from_bytes(keypair.secret_key().as_bytes());
    let client_id = DecodedClientId::from_key(&keypair.public_key());
    let client_did_key = format!("did:key:{client_id}");

    let account_signing_key = k256::ecdsa::SigningKey::random(&mut OsRng);
    let address = &Keccak256::default()
        .chain_update(
            &account_signing_key
                .verifying_key()
                .to_encoded_point(false)
                .as_bytes()[1..],
        )
        .finalize()[12..];
    let account = format!("eip155:1:0x{}", hex::encode(address));
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
                statement: Some(
                    "I further authorize this DAPP to send and receive messages on my behalf for \
                     this domain using my WalletConnect identity."
                        .to_owned(),
                ),
                aud: client_did_key.clone(),
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

    async fn create_client(
        relay_url: &str,
        relay_project_id: &str,
        keypair: &Keypair,
        notify_url: &str,
    ) -> (
        StaticSecret,
        x25519_dalek::PublicKey,
        Arc<relay_client::websocket::Client>,
        UnboundedReceiver<RelayClientEvent>,
    ) {
        let secret = StaticSecret::random_from_rng(OsRng);
        let public = PublicKey::from(&secret);

        // Create a websocket client to communicate with relay
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let connection_handler = wsclient::RelayConnectionHandler::new("notify-client", tx);
        let wsclient = Arc::new(relay_client::websocket::Client::new(connection_handler));

        let opts =
            wsclient::create_connection_opts(relay_url, relay_project_id, keypair, notify_url)
                .unwrap();
        wsclient.connect(&opts).await.unwrap();

        // Eat up the "connected" message
        _ = rx.recv().await.unwrap();

        (secret, public, wsclient, rx)
    }

    #[allow(clippy::too_many_arguments)]
    async fn watch_subscriptions(
        app_domain: &str,
        notify_url: &str,
        signing_key: &SigningKey,
        client_did_key: &str,
        did_pkh: &str,
        secret: &StaticSecret,
        public: &x25519_dalek::PublicKey,
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
                iss: client_did_key.to_owned(),
                act: "notify_watch_subscriptions".to_owned(),
                aud: client_did_key.to_owned(), // TODO should be dapp key not client_id
            },
            ksu: KEYS_SERVER.to_string(),
            sub: did_pkh.to_owned(),
            app: Some(format!("did:web:{app_domain}")),
        };

        let message = NotifyRequest::new(
            NOTIFY_WATCH_SUBSCRIPTIONS_METHOD,
            NotifyWatchSubscriptions {
                watch_subscriptions_auth: encode_auth(&subscription_auth, signing_key),
            },
        );

        let response_topic_key =
            derive_key(&x25519_dalek::PublicKey::from(key_agreement_key), secret).unwrap();
        let response_topic = sha256::digest(&response_topic_key);

        let envelope =
            Envelope::<EnvelopeType1>::new(&response_topic_key, message, *public.as_bytes())
                .unwrap();
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

        let decrypted_response =
            ChaCha20Poly1305::new(GenericArray::from_slice(&response_topic_key))
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

    let (secret, public, wsclient, mut rx) =
        create_client(&relay_url, &relay_project_id, &keypair, &notify_url).await;

    // ==== subscribe topic ====

    // TODO rename to "TEST_PROJECT_SECRET"
    let project_secret =
        std::env::var("NOTIFY_PROJECT_SECRET").expect("NOTIFY_PROJECT_SECRET not set");

    // Register project - generating subscribe topic
    let subscribe_topic_response = reqwest::Client::new()
        .post(format!("{notify_url}/{project_id}/subscribe-topic"))
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
            app_domain,
            &notify_url,
            &signing_key,
            &client_did_key,
            &did_pkh,
            &secret,
            &public,
            &wsclient,
            &mut rx,
        )
        .await;

        assert!(subs.is_empty());

        watch_topic_key
    };

    // Get app public key
    // TODO use struct
    let dapp_pubkey = subscribe_topic_response_body
        .get("subscribeKey")
        .unwrap()
        .as_str()
        .unwrap();

    // TODO use struct
    let dapp_identity_pubkey = subscribe_topic_response_body
        .get("authenticationKey")
        .unwrap()
        .as_str()
        .unwrap();

    // Get subscribe topic for dapp
    let subscribe_topic = sha256::digest(hex::decode(dapp_pubkey).unwrap().as_slice());

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
            iss: format!("did:key:{client_id}"),
            act: "notify_subscription".to_owned(),
            aud: format!("did:key:{client_id}"), // TODO should be dapp key not client_id
        },
        ksu: KEYS_SERVER.to_string(),
        sub: did_pkh.clone(),
        scp: "test test1".to_owned(),
        app: format!("did:web:{app_domain}"),
    };

    // Encode the subscription auth
    let subscription_auth = encode_auth(&subscription_auth, &signing_key);

    let sub_auth = json!({ "subscriptionAuth": subscription_auth });
    let message = NotifyRequest::new(NOTIFY_SUBSCRIBE_METHOD, sub_auth);

    let response_topic_key = derive_key(
        &x25519_dalek::PublicKey::from(decode_key(dapp_pubkey).unwrap()),
        &secret,
    )
    .unwrap();

    let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(&response_topic_key));

    let envelope =
        Envelope::<EnvelopeType1>::new(&response_topic_key, message, *public.as_bytes()).unwrap();
    let message = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    // Get response topic for wallet client and notify communication
    let response_topic = sha256::digest(&response_topic_key);

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
    assert_eq!(msg.tag, NOTIFY_SUBSCRIBE_RESPONSE_TAG);

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
        // assert_eq!(
        //     sub.scope,
        //     HashSet::from(["test".to_owned(), "test1".to_owned()])
        // );
        assert_eq!(sub.account, account);
        assert_eq!(sub.app_domain, app_domain);
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
    assert_eq!(msg.tag, 4050);

    let notification = Notification {
        title: "string".to_owned(),
        body: "string".to_owned(),
        icon: "string".to_owned(),
        url: "string".to_owned(),
        r#type: "test".to_owned(),
    };

    let notify_body = json!({
        "notification": notification,
        "accounts": [account]
    });

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
        dapp_identity_pubkey,
    )
    .unwrap();

    // https://github.com/WalletConnect/walletconnect-docs/blob/main/docs/specs/clients/notify/notify-authentication.md#notify-message
    // TODO: verify issuer
    assert_eq!(*claims.msg, notification);
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
            iss: format!("did:key:{}", client_id),
            act: "notify_update".to_owned(),
            aud: format!("did:key:{}", client_id), // TODO should be dapp key not client_id
        },
        ksu: KEYS_SERVER.to_string(),
        sub: did_pkh.clone(),
        scp: "test test2 test3".to_owned(),
        app: format!("did:web:{app_domain}"),
    };

    // Encode the subscription auth
    let update_auth = encode_auth(&update_auth, &signing_key);

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
    assert_eq!(claims.shared_claims.aud, format!("did:key:{}", client_id));
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
        // let subs = &auth.sbs[0];
        // assert_eq!(
        //     subs.scope,
        //     HashSet::from(["test".to_owned(), "test2".to_owned(),
        // "test3".to_owned()]) );
    }

    // Prepare deletion auth for *wallet* client
    // https://github.com/WalletConnect/walletconnect-docs/blob/main/docs/specs/clients/notify/notify-authentication.md#notify-delete
    let now = Utc::now();
    let delete_auth = SubscriptionDeleteRequestAuth {
        shared_claims: SharedClaims {
            iat: now.timestamp() as u64,
            exp: add_ttl(now, NOTIFY_DELETE_TTL).timestamp() as u64,
            iss: format!("did:key:{}", client_id),
            aud: format!("did:key:{}", client_id), // TODO should be dapp key not client_id
            act: "notify_delete".to_owned(),
        },
        ksu: KEYS_SERVER.to_string(),
        sub: did_pkh.clone(),
        app: format!("did:web:{app_domain}"),
    };

    // Encode the subscription auth
    let delete_auth = encode_auth(&delete_auth, &signing_key);
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
    assert_eq!(claims.shared_claims.aud, format!("did:key:{}", client_id));
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
        .json::<notify_server::handlers::notify::Response>()
        .await
        .unwrap();

    assert_eq!(resp.not_found.len(), 1);

    let unregister_auth = UnregisterIdentityRequestAuth {
        shared_claims: SharedClaims {
            iat: Utc::now().timestamp() as u64,
            exp: Utc::now().timestamp() as u64 + 3600,
            iss: client_did_key,
            aud: KEYS_SERVER.to_string(),
            act: "unregister_identity".to_owned(),
        },
        pkh: did_pkh,
    };
    let unregister_auth = encode_auth(&unregister_auth, &signing_key);
    reqwest::Client::new()
        .delete(KEYS_SERVER.join("/identity").unwrap())
        .body(serde_json::to_string(&json!({"idAuth": unregister_auth})).unwrap())
        .send()
        .await
        .unwrap();
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

// Workaround https://github.com/rust-lang/rust-clippy/issues/11613
#[allow(clippy::needless_return_with_question_mark)]
fn verify_jwt(jwt: &str, key: &str) -> notify_server::error::Result<JwtMessage> {
    // Refactor to call from_jwt() and then check `iss` with:
    // let pub_key = did_key.parse::<DecodedClientId>()?;
    // let key = jsonwebtoken::DecodingKey::from_ed_der(pub_key.as_ref());
    // Or perhaps do the opposite (i.e. serialize key into iss)

    let key = jsonwebtoken::DecodingKey::from_ed_der(&hex::decode(key).unwrap());

    let mut parts = jwt.rsplitn(2, '.');

    let (Some(signature), Some(message)) = (parts.next(), parts.next()) else {
        return Err(AuthError::Format)?;
    };

    // Finally, verify signature.
    let sig_result = jsonwebtoken::crypto::verify(
        signature,
        message.as_bytes(),
        &key,
        jsonwebtoken::Algorithm::EdDSA,
    );

    match sig_result {
        Ok(true) => Ok(serde_json::from_slice::<JwtMessage>(
            &base64::engine::general_purpose::STANDARD_NO_PAD
                .decode(jwt.split('.').nth(1).unwrap())
                .unwrap(),
        )?),
        Ok(false) | Err(_) => Err(AuthError::InvalidSignature)?,
    }
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
