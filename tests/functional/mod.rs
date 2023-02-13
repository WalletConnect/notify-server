use {
    crate::context::ServerContext,
    cast_server::{
        handlers::{notify::NotifyBody, register::RegisterBody},
        jsonrpc::Notification,
    },
    chacha20poly1305::KeyInit,
    test_context::test_context,
};

// #[test_context(ServerContext)]
// #[tokio::test]
// async fn test_health(ctx: &mut ServerContext) {
//     let body = reqwest::get(format!("http://{}/health", ctx.server.public_addr))
//         .await
//         .expect("Failed to call /health")
//         .status();
//     assert!(body.is_success());
// }

// #[test_context(ServerContext)]
// #[tokio::test]
// async fn test_register(ctx: &mut ServerContext) {
//     let client = reqwest::Client::new();
//     let key =
//         chacha20poly1305::ChaCha20Poly1305::generate_key(&mut
// chacha20poly1305::aead::OsRng {});     let hex_key = hex::encode(key);

//     let body = RegisterBody {
//         account: "test_account".to_owned(),
//         relay_url: ctx.relay_url.clone(),
//         sym_key: hex_key,
//     };

//     let status = client
//         .post(format!(
//             "http://{}/{}/register",
//             ctx.server.public_addr, ctx.project_id
//         ))
//         .body(serde_json::to_string(&body).unwrap())
//         .header("Content-Type", "application/json")
//         .send()
//         .await
//         .expect("Failed to call /health")
//         .status();
//     assert!(status.is_success());
// }

// #[test_context(ServerContext)]
// #[tokio::test]
// async fn test_notify(ctx: &mut ServerContext) {
//     let client = reqwest::Client::new();
//     let key =
//         chacha20poly1305::ChaCha20Poly1305::generate_key(&mut
// chacha20poly1305::aead::OsRng {});     let hex_key = hex::encode(key);

//     let test_account = "test_account".to_owned();

//     let body = RegisterBody {
//         account: test_account.clone(),
//         relay_url: ctx.relay_url.clone(),
//         sym_key: hex_key,
//     };

//     let status = client
//         .post(format!(
//             "http://{}/{}/register",
//             ctx.server.public_addr, ctx.project_id
//         ))
//         .body(serde_json::to_string(&body).unwrap())
//         .header("Content-Type", "application/json")
//         .send()
//         .await
//         .expect("Failed to call /register")
//         .status();
//     assert!(status.is_success());

//     let notification = Notification {
//         title: "test".to_owned(),
//         body: "test".to_owned(),
//         icon: "test".to_owned(),
//         url: "test".to_owned(),
//     };

//     let body = NotifyBody {
//         notification,
//         accounts: vec![test_account, "invalid_account".to_string()],
//     };

//     let response = client
//         .post(format!(
//             "http://{}/{}/notify",
//             ctx.server.public_addr, ctx.project_id
//         ))
//         .body(serde_json::to_string(&body).unwrap())
//         .header("Content-Type", "application/json")
//         .send()
//         .await
//         .expect("Failed to call /notify")
//         .text()
//         .await;
//     dbg!(response);
// }
