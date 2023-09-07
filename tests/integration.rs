use {
    base64::Engine,
    chacha20poly1305::{
        aead::{generic_array::GenericArray, Aead, OsRng},
        ChaCha20Poly1305,
        KeyInit,
    },
    chrono::Utc,
    ed25519_dalek::{Signer, SigningKey},
    hyper::StatusCode,
    lazy_static::lazy_static,
    notify_server::{
        auth::{
            add_ttl,
            from_jwt,
            AuthError,
            GetSharedClaims,
            SharedClaims,
            SubscriptionDeleteResponseAuth,
            SubscriptionRequestAuth,
            SubscriptionResponseAuth,
            SubscriptionUpdateRequestAuth,
            SubscriptionUpdateResponseAuth,
            SubscruptionDeleteRequestAuth,
        },
        handlers::notify::JwtMessage,
        jsonrpc::NotifyPayload,
        spec::{
            NOTIFY_DELETE_RESPONSE_TAG,
            NOTIFY_DELETE_TAG,
            NOTIFY_DELETE_TTL,
            NOTIFY_MESSAGE_TAG,
            NOTIFY_SUBSCRIBE_RESPONSE_TAG,
            NOTIFY_SUBSCRIBE_TAG,
            NOTIFY_SUBSCRIBE_TTL,
            NOTIFY_UPDATE_RESPONSE_TAG,
            NOTIFY_UPDATE_TAG,
            NOTIFY_UPDATE_TTL,
        },
        types::{Envelope, EnvelopeType0, EnvelopeType1, Notification},
        websocket_service::{NotifyMessage, NotifyResponse},
        wsclient::{self, RelayClientEvent},
    },
    rand::{rngs::StdRng, Rng, SeedableRng},
    relay_rpc::{
        auth::{
            cacao::{self, signature::Eip191},
            ed25519_dalek::Keypair,
        },
        domain::{ClientId, DecodedClientId},
        jwt::{JwtHeader, JWT_HEADER_ALG, JWT_HEADER_TYP},
    },
    serde::Serialize,
    serde_json::json,
    sha2::{Digest, Sha256},
    sha3::Keccak256,
    std::sync::Arc,
    url::Url,
    x25519_dalek::{PublicKey, StaticSecret},
};

const JWT_LEEWAY: i64 = 30;

lazy_static! {
    static ref KEYS_SERVER: Url = "https://staging.keys.walletconnect.com".parse().unwrap();
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
    let project_id =
        std::env::var("TEST_PROJECT_ID").expect("Tests requires TEST_PROJECT_ID to be set");

    let (notify_url, relay_url) = urls(env);

    // Generate valid JWT
    let mut rng = StdRng::from_entropy();
    let keypair = Keypair::generate(&mut rng);
    let signing_key = SigningKey::from_bytes(keypair.secret_key().as_bytes());

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

    let seed: [u8; 32] = rng.gen();

    let secret = StaticSecret::from(seed);
    let public = PublicKey::from(&secret);

    // Set up clients
    let http_client = reqwest::Client::new();

    // Create a websocket client to communicate with relay
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    let connection_handler = wsclient::RelayConnectionHandler::new("notify-client", tx);
    let wsclient = Arc::new(relay_client::websocket::Client::new(connection_handler));

    let opts =
        wsclient::create_connection_opts(&relay_url, &project_id, &keypair, &notify_url).unwrap();
    wsclient.connect(&opts).await.unwrap();

    // Eat up the "connected" message
    _ = rx.recv().await.unwrap();

    // TODO rename to "TEST_PROJECT_SECRET"
    let project_secret =
        std::env::var("NOTIFY_PROJECT_SECRET").expect("NOTIFY_PROJECT_SECRET not set");

    // Register project - generating subscribe topic
    let subscribe_topic_response = http_client
        .post(format!("{}/{}/subscribe-topic", &notify_url, &project_id))
        .bearer_auth(&project_secret)
        .json(&json!({ "dappUrl": "https://my-test-app.com" }))
        .send()
        .await
        .unwrap();
    assert_eq!(subscribe_topic_response.status(), StatusCode::OK);
    let subscribe_topic_response_body: serde_json::Value =
        subscribe_topic_response.json().await.unwrap();

    // Get app public key
    // TODO use struct
    let dapp_pubkey = subscribe_topic_response_body
        .get("subscribeTopicPublicKey")
        .unwrap()
        .as_str()
        .unwrap();

    // TODO use struct
    let dapp_identity_pubkey = subscribe_topic_response_body
        .get("identityPublicKey")
        .unwrap()
        .as_str()
        .unwrap();

    // Get subscribe topic for dapp
    let subscribe_topic = sha256::digest(&*hex::decode(dapp_pubkey).unwrap());

    let decoded_client_id = DecodedClientId(*keypair.public_key().as_bytes());
    let client_id = ClientId::from(decoded_client_id);
    let did_key = format!("did:key:{}", client_id);

    // Register identity key with keys server
    let mut cacao = cacao::Cacao {
        h: cacao::header::Header {
            t: "eip4361".to_owned(),
        },
        p: cacao::payload::Payload {
            domain: "app.example.com".to_owned(),
            iss: did_pkh.clone(),
            statement: None, // TODO add statement
            aud: did_key.clone(),
            version: cacao::Version::V1,
            nonce: "xxxx".to_owned(), // TODO
            iat: Utc::now().to_rfc3339(),
            exp: None,
            nbf: None,
            request_id: None,
            resources: None, // TODO add identity.walletconnect.com
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
            iss: format!("did:key:{}", client_id),
        },
        ksu: KEYS_SERVER.to_string(),
        sub: did_pkh.clone(),
        aud: format!("did:key:{}", client_id), // TODO should be dapp key not client_id
        scp: "test test1".to_owned(),
        act: "notify_subscription".to_owned(),
        app: "https://my-test-app.com".to_owned(),
    };

    // Encode the subscription auth
    let subscription_auth = encode_auth(&subscription_auth, &signing_key);
    let sub_auth_hash = sha256::digest(&*subscription_auth.clone());

    let id = chrono::Utc::now().timestamp_millis().unsigned_abs();

    let id = id * 1000 + rand::thread_rng().gen_range(100, 1000);

    let sub_auth = json!({ "subscriptionAuth": subscription_auth });
    let message = json!({"id": id,  "jsonrpc": "2.0", "params": sub_auth});

    let response_topic_key = derive_key(dapp_pubkey.to_string(), hex::encode(secret.to_bytes()));

    let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(
        &hex::decode(response_topic_key.clone()).unwrap(),
    ));

    let envelope =
        Envelope::<EnvelopeType1>::new(&response_topic_key, message, *public.as_bytes()).unwrap();
    let message = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

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

    // Get response topic for wallet client and notify communication
    let response_topic = sha256::digest(&*hex::decode(response_topic_key.clone()).unwrap());

    // Subscribe to the topic and listen for response
    // No race condition to subscribe after publishing due to shared mailbox
    wsclient
        .subscribe(response_topic.clone().into())
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
    let pubkey = DecodedClientId::try_from_did_key(&subscribe_response_auth.sub).unwrap();

    let notify_key = derive_key(hex::encode(pubkey), hex::encode(secret.to_bytes()));
    let notify_topic = sha256::digest(&*hex::decode(&notify_key).unwrap());

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

    let _res = http_client
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

    let cipher =
        ChaCha20Poly1305::new(GenericArray::from_slice(&hex::decode(&notify_key).unwrap()));

    let Envelope::<EnvelopeType0> { iv, sealbox, .. } = Envelope::<EnvelopeType0>::from_bytes(
        base64::engine::general_purpose::STANDARD
            .decode(msg.message.as_bytes())
            .unwrap(),
    )
    .unwrap();

    // TODO: add proper type for that val
    let decrypted_notification: NotifyMessage<NotifyPayload> = serde_json::from_slice(
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
    assert_eq!(claims.msg, notification);
    assert_eq!(claims.sub, sub_auth_hash);
    assert!(claims.iat < chrono::Utc::now().timestamp() + JWT_LEEWAY); // TODO remove leeway
    assert!(claims.exp > chrono::Utc::now().timestamp() - JWT_LEEWAY); // TODO remove leeway
    assert_eq!(claims.app, "https://my-test-app.com");
    assert_eq!(claims.aud, did_pkh);
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
        },
        ksu: KEYS_SERVER.to_string(),
        sub: did_pkh.clone(),
        aud: format!("did:key:{}", client_id), // TODO should be dapp key not client_id
        scp: "test test2 test3".to_owned(),
        act: "notify_update".to_owned(),
        app: "https://my-test-app.com".to_owned(),
    };

    // Encode the subscription auth
    let update_auth = encode_auth(&update_auth, &signing_key);
    let update_auth_hash = sha256::digest(&*update_auth.clone());

    let sub_auth = json!({ "updateAuth": update_auth });

    let delete_message = json!({
        "id": id,
        "jsonrpc": "2.0",
        "params": sub_auth,
    });

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
    assert_eq!(claims.sub, update_auth_hash);
    assert!((claims.shared_claims.iat as i64) < chrono::Utc::now().timestamp() + JWT_LEEWAY); // TODO remove leeway
    assert!((claims.shared_claims.exp as i64) > chrono::Utc::now().timestamp() - JWT_LEEWAY); // TODO remove leeway
    assert_eq!(claims.app, "https://my-test-app.com");
    assert_eq!(claims.aud, format!("did:key:{}", client_id));
    assert_eq!(claims.act, "notify_update_response");

    // Prepare deletion auth for *wallet* client
    // https://github.com/WalletConnect/walletconnect-docs/blob/main/docs/specs/clients/notify/notify-authentication.md#notify-delete
    let now = Utc::now();
    let delete_auth = SubscruptionDeleteRequestAuth {
        shared_claims: SharedClaims {
            iat: now.timestamp() as u64,
            exp: add_ttl(now, NOTIFY_DELETE_TTL).timestamp() as u64,
            iss: format!("did:key:{}", client_id),
        },
        ksu: KEYS_SERVER.to_string(),
        sub: did_pkh.clone(),
        aud: format!("did:key:{}", client_id), // TODO should be dapp key not client_id
        act: "notify_delete".to_owned(),
        app: "https://my-test-app.com".to_owned(),
    };

    // Encode the subscription auth
    let delete_auth = encode_auth(&delete_auth, &signing_key);
    let _delete_auth_hash = sha256::digest(&*delete_auth.clone());

    let sub_auth = json!({ "deleteAuth": delete_auth });

    let delete_message = json!({
        "id": id,
        "jsonrpc": "2.0",
        "params": sub_auth,
    });

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
    assert_eq!(claims.sub, update_auth_hash);
    assert!((claims.shared_claims.iat as i64) < chrono::Utc::now().timestamp() + JWT_LEEWAY); // TODO remove leeway
    assert!((claims.shared_claims.exp as i64) > chrono::Utc::now().timestamp() - JWT_LEEWAY); // TODO remove leeway
    assert_eq!(claims.app, "https://my-test-app.com");
    assert_eq!(claims.aud, format!("did:key:{}", client_id));
    assert_eq!(claims.act, "notify_delete_response");

    // wait for notify server to unregister the user
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    let resp = http_client
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
            iss: did_key,
        },
        pkh: did_pkh,
        aud: KEYS_SERVER.to_string(),
        act: "unregister_identity".to_owned(),
    };
    let unregister_auth = encode_auth(&unregister_auth, &signing_key);
    reqwest::Client::builder()
        .build()
        .unwrap()
        .delete(KEYS_SERVER.join("/identity").unwrap())
        .body(serde_json::to_string(&json!({"idAuth": unregister_auth})).unwrap())
        .send()
        .await
        .unwrap();
}

fn derive_key(pubkey: String, privkey: String) -> String {
    let pubkey: [u8; 32] = hex::decode(pubkey).unwrap()[..32].try_into().unwrap();
    let privkey: [u8; 32] = hex::decode(privkey).unwrap()[..32].try_into().unwrap();

    let secret_key = x25519_dalek::StaticSecret::from(privkey);
    let public_key = x25519_dalek::PublicKey::from(pubkey);

    let shared_key = secret_key.diffie_hellman(&public_key);
    let derived_key = hkdf::Hkdf::<Sha256>::new(None, shared_key.as_bytes());

    let mut expanded_key = [0u8; 32];
    derived_key.expand(b"", &mut expanded_key).unwrap();

    hex::encode(expanded_key)
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
    /// description of action intent. Must be equal to "unregister_identity"
    pub act: String,
    /// keyserver URL
    pub aud: String,
    /// corresponding blockchain account (did:pkh)
    pub pkh: String,
}

impl GetSharedClaims for UnregisterIdentityRequestAuth {
    fn get_shared_claims(&self) -> &SharedClaims {
        &self.shared_claims
    }
}
