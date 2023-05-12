use {
    crate::context::{encode_subscription_auth, ServerContext},
    cast_server::{auth::SubscriptionAuth, types::RegisterBody},
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
async fn test_register(ctx: &mut ServerContext) {
    let client = reqwest::Client::new();
    let key =
        chacha20poly1305::ChaCha20Poly1305::generate_key(&mut chacha20poly1305::aead::OsRng {});
    let keypair = Keypair::generate(&mut OsRng {});
    let hex_key = hex::encode(key);

    let decoded_client_id = DecodedClientId(*keypair.public_key().as_bytes());
    let client_id = ClientId::from(decoded_client_id);

    let subscription_auth = SubscriptionAuth {
        iat: Utc::now().timestamp() as u64,
        exp: Utc::now().timestamp() as u64 + 3600,
        iss: format!("did:key:{}", client_id),
        ksu: "https://keys.walletconnect.com".to_owned(),
        sub: "did:pkh:test_account".to_owned(),
        aud: "https://my-test-app.com".to_owned(),
        scp: "test test1".to_owned(),
        act: "push_subscription".to_owned(),
    };

    let subscription_auth = encode_subscription_auth(&subscription_auth, &keypair);

    // Fix the url for register body
    let relay_url = ctx.relay_url.replace("http", "ws");

    let body = RegisterBody {
        account: "test_account".to_owned(),
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
}
