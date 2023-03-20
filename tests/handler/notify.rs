use {
    crate::context::ServerContext,
    cast_server::{handlers::notify::NotifyBody, jsonrpc::Notification, types::RegisterBody},
    chacha20poly1305::KeyInit,
    test_context::test_context,
};

#[test_context(ServerContext)]
#[tokio::test]
async fn test_notify(ctx: &mut ServerContext) {
    let client = reqwest::Client::new();
    let key =
        chacha20poly1305::ChaCha20Poly1305::generate_key(&mut chacha20poly1305::aead::OsRng {});
    let hex_key = hex::encode(key);

    let test_account = "test_account".to_owned();
    let test_account_invalid_relay = "test_account_fail".to_owned();

    // Fix the url for register body
    let relay_url = ctx.relay_url.replace("http", "ws").trim().to_string();

    // Create valid account
    let body = RegisterBody {
        account: test_account.clone(),
        relay_url,
        sym_key: hex_key.clone(),
    };

    // Register valid account
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

    // Prepare invalid account
    let body = RegisterBody {
        account: test_account_invalid_relay.clone(),
        relay_url: "wss://not-a-relay.com".to_string(),
        sym_key: hex_key,
    };

    // Register account with invalid relay
    let status = client
        .post(format!(
            "http://{}/{}/register",
            ctx.server.public_addr, ctx.project_id
        ))
        .body(serde_json::to_string(&body).unwrap())
        .header("Content-Type", "application/json")
        .send()
        .await
        .expect("Failed to call /register")
        .status();
    assert!(status.is_success());

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
        accounts: vec![
            test_account,
            test_account_invalid_relay,
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
        "test_account_fail"
    );
    assert_eq!(response.not_found.len(), 1);
}
