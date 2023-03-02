use {
    base64::Engine,
    cast_server::{
        auth::jwt_token,
        handlers::{
            notify::{Envelope, NotifyBody},
            register::RegisterBody,
        },
        jsonrpc::{JsonRpcParams, JsonRpcPayload, Notification},
        wsclient,
    },
    chacha20poly1305::{
        aead::{generic_array::GenericArray, AeadMut},
        KeyInit,
    },
    rand::{rngs::StdRng, SeedableRng},
    walletconnect_sdk::rpc::rpc::{Params, Payload},
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
    let seed: [u8; 32] = "THIS_IS_TEST_VALUE_SHOULD_NOT_BE_USED_IN_PROD"
        .to_string()
        .as_bytes()[..32]
        .try_into()
        .unwrap();
    let mut seeded = StdRng::from_seed(seed);
    let keypair = ed25519_dalek::Keypair::generate(&mut seeded);
    let jwt = jwt_token(&relay_url, &keypair);

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

    let test_account = "test_account".to_owned();

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
