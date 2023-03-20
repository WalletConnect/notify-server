use {
    base64::Engine,
    cast_server::{
        auth::jwt_token,
        handlers::notify::{Envelope, NotifyBody},
        jsonrpc::{JsonRpcParams, JsonRpcPayload, Notification},
        types::RegisterBody,
        wsclient,
    },
    chacha20poly1305::{
        aead::{generic_array::GenericArray, AeadMut},
        consts::U12,
        KeyInit,
    },
    rand::{distributions::Uniform, prelude::Distribution, rngs::StdRng, SeedableRng},
    std::time::Duration,
    tokio::time::sleep,
    walletconnect_sdk::rpc::{
        auth::ed25519_dalek::Keypair,
        rpc::{Params, Payload},
    },
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
        _ => panic!("Invalid environment"),
    }
}

#[tokio::test]
async fn cast_properly_sending_message() {
    let env = std::env::var("ENVIRONMENT").unwrap_or("STAGING".to_owned());
    let project_id = std::env::var("PROJECT_ID").expect("Tests requires PROJECT_ID to be set");

    let (cast_url, relay_url) = urls(env);

    // Generate valid JWT
    let mut rng = StdRng::from_entropy();
    let keypair = Keypair::generate(&mut rng);
    let jwt = jwt_token(&relay_url, &keypair).unwrap();

    // Set up clients
    let http_client = reqwest::Client::new();
    let mut ws_client = wsclient::connect(&relay_url, &project_id, jwt)
        .await
        .unwrap();

    // Prepare client key
    let key =
        chacha20poly1305::ChaCha20Poly1305::generate_key(&mut chacha20poly1305::aead::OsRng {});
    let topic = sha256::digest(&*key);
    let hex_key = hex::encode(key);

    let test_account = "test_account_send_test".to_owned();

    // Create valid account
    let body = RegisterBody {
        account: test_account.clone(),
        relay_url,
        sym_key: hex_key.clone(),
    };

    // Register valid account
    let status = http_client
        .post(format!("{}/{}/register", &cast_url, &project_id))
        .body(serde_json::to_string(&body).unwrap())
        .header("Content-Type", "application/json")
        .send()
        .await
        .expect("Failed to call /register")
        .status();
    assert!(status.is_success());

    // Prepare notification
    let test_notification = Notification {
        title: "test".to_owned(),
        body: "test".to_owned(),
        icon: "test".to_owned(),
        url: "test".to_owned(),
    };

    // Prepare notify body
    let body = NotifyBody {
        notification: test_notification.clone(),
        accounts: vec![test_account.clone()],
    };

    // Subscribe client to topic
    ws_client.subscribe(&topic).await.unwrap();

    // Receive ack to subscribe
    ws_client.recv().await.unwrap();

    // Send notify
    let response = http_client
        .post(format!("{}/{}/notify", &cast_url, &project_id))
        .body(serde_json::to_string(&body).unwrap())
        .header("Content-Type", "application/json")
        .send()
        .await
        .expect("Failed to call /notify")
        .text()
        .await
        .unwrap();
    let response: cast_server::handlers::notify::Response =
        serde_json::from_str(&response).unwrap();

    assert_eq!(response.sent.into_iter().next().unwrap(), test_account);

    // Recv the response
    if let Payload::Request(request) = ws_client.recv().await.unwrap() {
        if let Params::Subscription(params) = request.params {
            assert_eq!(params.data.topic, topic.into());
            let mut cipher =
                chacha20poly1305::ChaCha20Poly1305::new(GenericArray::from_slice(&key));
            let encrypted_text = base64::engine::general_purpose::STANDARD
                .decode(&*params.data.message)
                .unwrap();
            let envelope = Envelope::from_bytes(encrypted_text);
            let decrypted = cipher
                .decrypt(&envelope.iv.into(), &*envelope.sealbox)
                .unwrap();

            let push_message: JsonRpcPayload = serde_json::from_slice(&decrypted).unwrap();
            if let JsonRpcParams::Push(notification) = push_message.params {
                assert_eq!(notification, test_notification);
            } else {
                panic!("Notification received not matching notification sent")
            }
        } else {
            panic!("Wrong payload body received");
        }
    } else {
        panic!("Invalid data received");
    }
}

#[tokio::test]
async fn test_unregister() {
    let env = std::env::var("ENVIRONMENT").unwrap_or("STAGING".to_owned());
    let project_id = std::env::var("PROJECT_ID").expect("Tests requires PROJECT_ID to be set");

    let (cast_url, relay_url) = urls(env);

    let client = reqwest::Client::new();
    let key =
        chacha20poly1305::ChaCha20Poly1305::generate_key(&mut chacha20poly1305::aead::OsRng {});

    let mut rng = StdRng::from_entropy();

    let keypair = Keypair::generate(&mut rng);
    let jwt = jwt_token(&relay_url, &keypair).unwrap();

    let mut ws_client = wsclient::connect(&relay_url, &project_id, jwt.clone())
        .await
        .unwrap();

    let hex_key = hex::encode(key);

    let test_account = "test_account1".to_owned();

    // Create valid account
    let body = RegisterBody {
        account: test_account.clone(),
        relay_url: relay_url.clone(),
        sym_key: hex_key.clone(),
    };

    // Register valid account
    let status = client
        .post(format!("{}/{}/register", &cast_url, &project_id))
        .body(serde_json::to_string(&body).unwrap())
        .header("Content-Type", "application/json")
        .send()
        .await
        .expect("Failed to call /register");
    assert!(status.status().is_success());

    //  Send unregister on websocket
    let topic = sha256::digest(key.as_slice());

    let encryption_key = hex::decode(hex_key).unwrap();

    let mut cipher =
        chacha20poly1305::ChaCha20Poly1305::new(GenericArray::from_slice(&encryption_key));

    let uniform = Uniform::from(0u8..=255);

    let mut rng = StdRng::from_entropy();

    let nonce: GenericArray<u8, U12> =
        GenericArray::from_iter(uniform.sample_iter(&mut rng).take(12));

    let message = "{\"code\": 3,\"message\": \"Unregister reason\"}";

    let encrypted = cipher.encrypt(&nonce, message.clone().as_bytes()).unwrap();

    let envelope = Envelope {
        envelope_type: 0,
        iv: nonce.into(),
        sealbox: encrypted,
    };

    let encrypted_msg = envelope.to_bytes();

    let message = base64::engine::general_purpose::STANDARD.encode(encrypted_msg);
    ws_client.subscribe(&topic).await.unwrap();
    ws_client
        .publish_with_tag(&topic, &message, 4004)
        .await
        .unwrap();

    // Time for relay to process message
    sleep(Duration::from_secs(5)).await;

    // Prepare notification
    let notification = Notification {
        title: "test".to_owned(),
        body: "test".to_owned(),
        icon: "test".to_owned(),
        url: "test".to_owned(),
    };

    // Prepare notify body
    let body = NotifyBody {
        notification,
        accounts: vec![test_account],
    };

    // Send notify
    let response = client
        .post(format!("{}/{}/notify", &cast_url, &project_id))
        .body(serde_json::to_string(&body).unwrap())
        .header("Content-Type", "application/json")
        .send()
        .await
        .expect("Failed to call /notify")
        .text()
        .await
        .unwrap();

    let response: cast_server::handlers::notify::Response =
        serde_json::from_str(&response).unwrap();

    // Assert that account was deleted and therefore response is not sent
    assert_eq!(response.sent.len(), 0);
}
