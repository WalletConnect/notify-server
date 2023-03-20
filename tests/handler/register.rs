use {
    crate::context::ServerContext,
    cast_server::types::RegisterBody,
    chacha20poly1305::KeyInit,
    test_context::test_context,
};

#[test_context(ServerContext)]
#[tokio::test]
async fn test_register(ctx: &mut ServerContext) {
    let client = reqwest::Client::new();
    let key =
        chacha20poly1305::ChaCha20Poly1305::generate_key(&mut chacha20poly1305::aead::OsRng {});
    let hex_key = hex::encode(key);

    // Fix the url for register body
    let relay_url = ctx.relay_url.replace("http", "ws");

    let body = RegisterBody {
        account: "test_account".to_owned(),
        relay_url,
        sym_key: hex_key,
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
