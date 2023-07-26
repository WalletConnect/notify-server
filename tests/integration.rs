use {
    crate::context::encode_subscription_auth,
    base64::Engine,
    cast_server::{
        auth::SubscriptionAuth,
        types::{Envelope, EnvelopeType0, EnvelopeType1, Notification},
        websocket_service::{NotifyMessage, NotifyResponse},
        wsclient::{self, RelayClientEvent},
    },
    chacha20poly1305::{
        aead::{generic_array::GenericArray, AeadMut},
        ChaCha20Poly1305,
        KeyInit,
    },
    chrono::Utc,
    rand::{rngs::StdRng, Rng, SeedableRng},
    relay_rpc::{
        auth::ed25519_dalek::Keypair,
        domain::{ClientId, DecodedClientId},
    },
    serde_json::json,
    sha2::Sha256,
    std::{sync::Arc, time::Duration},
    x25519_dalek::{PublicKey, StaticSecret},
};

mod context;

fn urls(env: String) -> (String, String) {
    match env.as_str() {
        "STAGING" => (
            "https://staging.cast.walletconnect.com".to_owned(),
            "wss://staging.relay.walletconnect.com".to_owned(),
        ),
        "PROD" => (
            "https://cast.walletconnect.com".to_owned(),
            "wss://relay.walletconnect.com".to_owned(),
        ),
        "LOCAL" => (
            "http://127.0.0.1:3000".to_owned(),
            "wss://staging.relay.walletconnect.com".to_owned(),
        ),
        _ => panic!("Invalid environment"),
    }
}

const TEST_ACCOUNT: &'static str = "eip155:123:test_account";

#[tokio::test]
async fn cast_properly_sending_message() {
    let env = std::env::var("ENVIRONMENT").unwrap_or("STAGING".to_owned());
    let project_id = std::env::var("TEST_PROJECT_ID").expect(
        "Tests requires
PROJECT_ID to be set",
    );

    let (cast_url, relay_url) = urls(env);

    // Generate valid JWT
    let mut rng = StdRng::from_entropy();
    let keypair = Keypair::generate(&mut rng);

    let seed: [u8; 32] = rng.gen();

    let secret = StaticSecret::from(seed);
    let public = PublicKey::from(&secret);

    // Set up clients
    let http_client = reqwest::Client::new();

    // Create a websocket client to communicate with relay
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    let connection_handler = wsclient::RelayConnectionHandler::new("cast-client", tx);
    let wsclient = Arc::new(relay_client::websocket::Client::new(connection_handler));

    let opts =
        wsclient::create_connection_opts(&relay_url, &project_id, &keypair, &cast_url).unwrap();
    wsclient.connect(&opts).await.unwrap();

    // Eat up the "connected" message
    _ = rx.recv().await.unwrap();

    let project_secret = uuid::Uuid::new_v4().to_string();

    // Register project - generating subscribe topic
    let dapp_pubkey_response: serde_json::Value = http_client
        .get(format!("{}/{}/subscribe-topic", &cast_url, &project_id))
        .bearer_auth(project_secret)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    // Get app public key
    let dapp_pubkey = dapp_pubkey_response
        .get("publicKey")
        .unwrap()
        .as_str()
        .unwrap();

    // Get subscribe topic for dapp
    let subscribe_topic = sha256::digest(&*hex::decode(dapp_pubkey).unwrap());

    let decoded_client_id = DecodedClientId(*keypair.public_key().as_bytes());
    let client_id = ClientId::from(decoded_client_id);

    // ----------------------------------------------------
    // SUBSCRIBE WALLET CLIENT TO DAPP THROUGHT CAST
    // ----------------------------------------------------

    // Prepare subscription auth for *wallet* client
    let subscription_auth = SubscriptionAuth {
        iat: Utc::now().timestamp() as u64,
        exp: Utc::now().timestamp() as u64 + 3600,
        iss: format!("did:key:{}", client_id),
        ksu: "https://keys.walletconnect.com".to_owned(),
        sub: format!("did:pkh:{TEST_ACCOUNT}"),
        aud: "https://my-test-app.com".to_owned(),
        scp: "test test1".to_owned(),
        act: "push_subscription".to_owned(),
    };

    // Encode the subscription auth
    let subscription_auth = encode_subscription_auth(&subscription_auth, &keypair);

    let id = chrono::Utc::now().timestamp_millis().unsigned_abs();

    let id = id * 1000 + rand::thread_rng().gen_range(100, 1000);

    let sub_auth = json!({ "subscriptionAuth": subscription_auth });
    let message = json!({"id": id,  "jsonrpc": "2.0", "params": sub_auth});

    let response_topic_key = derive_key(dapp_pubkey.to_string(), hex::encode(secret.to_bytes()));

    let mut cipher = ChaCha20Poly1305::new(GenericArray::from_slice(
        &hex::decode(response_topic_key.clone()).unwrap(),
    ));

    let envelope =
        Envelope::<EnvelopeType1>::new(&response_topic_key, message, *public.as_bytes()).unwrap();
    let message = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    // Send subscription request to cast
    wsclient
        .publish(
            subscribe_topic.into(),
            message,
            4006,
            Duration::from_secs(86400),
            false,
        )
        .await
        .unwrap();

    // Get response topic for wallet client and cast communication
    let response_topic = sha256::digest(&*hex::decode(response_topic_key.clone()).unwrap());

    // Subscribe to the topic and listen for response
    wsclient
        .subscribe(response_topic.clone().into())
        .await
        .unwrap();

    let resp = rx.recv().await.unwrap();
    // wsclient.fetch(response_topic.clone().into()).await.unwrap();
    let RelayClientEvent::Message(msg) = resp else {
        panic!("Expected message, got {:?}", resp);
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

    let pubkey = response.result.get("publicKey").unwrap().as_str().unwrap();

    let notify_key = derive_key(pubkey.to_string(), hex::encode(secret.to_bytes()));
    let notify_topic = sha256::digest(&*hex::decode(&notify_key).unwrap());

    wsclient
        .subscribe(notify_topic.clone().into())
        .await
        .unwrap();
    let notification = Notification {
        title: "string".to_owned(),
        body: "string".to_owned(),
        icon: "string".to_owned(),
        url: "string".to_owned(),
        r#type: "test".to_owned(),
    };

    let notify_body = json!({
        "notification": notification,
        "accounts": [TEST_ACCOUNT]
    });

    // wait for notify server to register the user
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    let _res = http_client
        .post(format!("{}/{}/notify", &cast_url, &project_id))
        .json(&notify_body)
        .send()
        .await
        .unwrap();

    let _consume_4050_noop = rx.recv().await.unwrap();
    let resp = rx.recv().await.unwrap();
    let RelayClientEvent::Message(msg) = resp else {
        panic!("Expected message, got {:?}", resp);
    };

    let mut cipher =
        ChaCha20Poly1305::new(GenericArray::from_slice(&hex::decode(notify_key).unwrap()));

    let Envelope::<EnvelopeType0> { iv, sealbox, .. } = Envelope::<EnvelopeType0>::from_bytes(
        base64::engine::general_purpose::STANDARD
            .decode(msg.message.as_bytes())
            .unwrap(),
    )
    .unwrap();

    let decrypted_notification: NotifyMessage<Notification> = serde_json::from_slice(
        &cipher
            .decrypt(&iv.into(), chacha20poly1305::aead::Payload::from(&*sealbox))
            .unwrap(),
    )
    .unwrap();

    let received_notification = decrypted_notification.params;

    assert_eq!(received_notification, notification);

    let delete_params = json!({
      "code": 400,
      "message": "test"
    });

    let delete_message = json! ({
            "id": id,
            "jsonrpc": "2.0",
            "params": base64::engine::general_purpose::STANDARD.encode(delete_params.to_string().as_bytes())
    });

    let envelope = Envelope::<EnvelopeType0>::new(&response_topic_key, delete_message).unwrap();

    let encoded_message = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    wsclient
        .publish(
            notify_topic.into(),
            encoded_message,
            4004,
            Duration::from_secs(86400),
            false,
        )
        .await
        .unwrap();

    // wait for notify server to unregister the user
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    let resp = http_client
        .post(format!("{}/{}/notify", &cast_url, &project_id))
        .json(&notify_body)
        .send()
        .await
        .unwrap();

    let resp = resp
        .json::<cast_server::handlers::notify::Response>()
        .await
        .unwrap();

    assert_eq!(resp.not_found.len(), 1);
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
