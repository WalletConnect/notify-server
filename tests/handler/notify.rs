use {
    crate::context::{encode_subscription_auth, ServerContext},
    cast_server::{
        auth::SubscriptionAuth,
        handlers::notify::NotifyBody,
        types::{Notification, RegisterBody},
    },
    chacha20poly1305::KeyInit,
    chrono::Utc,
    rand_core::OsRng,
    test_context::test_context,
    walletconnect_sdk::rpc::{
        auth::ed25519_dalek::Keypair,
        domain::{ClientId, DecodedClientId},
    },
};

#[test_context(ServerContext)]
#[tokio::test]
async fn test_notify(ctx: &mut ServerContext) {
    let client = reqwest::Client::new();
    let key =
        chacha20poly1305::ChaCha20Poly1305::generate_key(&mut chacha20poly1305::aead::OsRng {});
    let keypair = Keypair::generate(&mut OsRng {});
    let hex_key = hex::encode(key);

    let test_account = "eip155:123:test_account";
    let invalid_relay = "test_account_invalid_relay";

    let decoded_client_id = DecodedClientId(*keypair.public_key().as_bytes());
    let client_id = ClientId::from(decoded_client_id);

    let subscription_auth = SubscriptionAuth {
        iat: Utc::now().timestamp() as u64,
        exp: Utc::now().timestamp() as u64 + 3600,
        iss: format!("did:key:{}", client_id),
        ksu: "https://keys.walletconnect.com".to_owned(),
        sub: "did:pkh:eip155:123:test_account".to_owned(),
        aud: "https://my-test-app.com".to_owned(),
        scp: "test test1".to_owned(),
        act: "push_subscription".to_owned(),
    };

    let subscription_auth = encode_subscription_auth(&subscription_auth, &keypair);

    let relay_url = ctx.relay_url.replace("http", "ws");

    let body = RegisterBody {
        account: test_account.to_owned(),
        relay_url,
        sym_key: hex_key.clone(),
        subscription_auth,
    };

    let status = client
        .post(format!(
            "http://{}/{}/register",
            ctx.server.public_addr, ctx.project_id
        ))
        .body(serde_json::to_string(&body).unwrap())
        .header("Content-Type", "application/json")
        .send()
        .await
        .expect("Failed to call /register");

    assert!(status.status().is_success());

    let keypair = Keypair::generate(&mut OsRng {});

    let decoded_client_id = DecodedClientId(*keypair.public_key().as_bytes());
    let client_id = ClientId::from(decoded_client_id);

    let subscription_auth = SubscriptionAuth {
        iat: Utc::now().timestamp() as u64,
        exp: Utc::now().timestamp() as u64 + 3600,
        iss: format!("did:key:{}", client_id),
        ksu: "https://keys.walletconnect.com".to_owned(),
        sub: "did:pkh:eip155:123:test_account".to_owned(),
        aud: "https://my-test-app.com".to_owned(),
        scp: "test test1".to_owned(),
        act: "push_subscription".to_owned(),
    };

    let subscription_auth = encode_subscription_auth(&subscription_auth, &keypair);

    // Fix the url for register body
    let relay_url = "wss://invalid-relay.com".to_owned();

    let body = RegisterBody {
        account: invalid_relay.to_owned(),
        relay_url,
        sym_key: hex_key,
        subscription_auth,
    };

    let status = client
        .post(format!(
            "http://{}/{}/register",
            ctx.server.public_addr, ctx.project_id
        ))
        .body(serde_json::to_string(&body).unwrap())
        .header("Content-Type", "application/json")
        .send()
        .await
        .expect("Failed to call /register");

    assert!(status.status().is_success());

    // Prepare notification
    let notification = Notification {
        title: "test".to_owned(),
        body: "test".to_owned(),
        icon: "test".to_owned(),
        url: "test".to_owned(),
        r#type: "test".to_owned(),
    };

    // Prepare notify body
    let body = NotifyBody {
        notification,
        accounts: vec![
            test_account.to_string(),
            invalid_relay.to_string(),
            "invalid_account".to_string(),
        ],
    };

    // Send notify
    let response = client
        .post(format!(
            "http://{}/{}/notify",
            ctx.server.public_addr, ctx.project_id
        ))
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

    assert_eq!(response.sent.len(), 1);
    assert_eq!(
        response.failed.into_iter().next().unwrap().account,
        invalid_relay
    );
    assert_eq!(response.not_found.len(), 1);
}
