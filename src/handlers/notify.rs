use {
    super::Account,
    crate::{handlers::ClientData, state::AppState},
    axum::{extract, extract::State, http::StatusCode, response::IntoResponse, Json},
    base64::Engine,
    chacha20poly1305::{
        aead::{generic_array::GenericArray, OsRng},
        KeyInit,
    },
    hyper::HeaderMap,
    mongodb::bson::doc,
    serde::{Deserialize, Serialize},
    std::sync::Arc,
    tokio_stream::StreamExt,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct CastArgs {
    notification: Notification,
    accounts: Vec<Account>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Notification {
    title: String,
    body: String,
    icon: String,
    url: String,
}

struct Query {
    project_id: String,
}
type Topic = Arc<str>;

/// Data structure representing PublishParams.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PublishParams {
    /// Topic to publish to.
    pub topic: Topic,
    /// Message to publish.
    pub message: Arc<str>,
    /// Duration for which the message should be kept in the mailbox if it can't
    /// be delivered, in seconds.
    #[serde(rename = "ttl")]
    pub ttl_secs: u32,
    // #[serde(default, skip_serializing_if = "is_default")]
    /// A label that identifies what type of message is sent based on the RPC
    /// method used.
    pub tag: u32,
    /// A flag that identifies whether the server should trigger a notification
    /// webhook to a client through a push server.
    // #[serde(default, skip_serializing_if = "is_default")]
    pub prompt: bool,
}

#[derive(Serialize, Deserialize)]
pub struct JsonRpcPayload {
    id: String,
    jsonrpc: String,
    method: String,
    params: PublishParams,
}

pub async fn handler(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(cast_args): Json<CastArgs>,
) -> impl IntoResponse {
    let db = state.example_store.clone().database("cast");

    let project_id = headers.get("Auth").unwrap().to_str().unwrap();

    // Sanitize the input
    let clients_string = match cast_args.accounts.len() {
        0 => {
            // TODO: Error on 0 accounts
            todo!()
        }
        1 => cast_args.accounts[0].0.clone(),
        _ => {
            let mut temp = cast_args.accounts[0].0.clone();
            for account in &cast_args.accounts[1..] {
                temp = format!("{}|{}", temp, account.0);
            }
            format!("({})", temp)
        }
    };

    // let clients = HashMap::new();

    let token = "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSJ9.eyJpc3MiOiJkaWQ6a2V5Ono2TWt3RDh1cGl1czlCU3A2OVRSRXJGd1NnVUNWaERxTXFabVFDQW9CQ2pCRGNzUyIsInN1YiI6ImRjZWE1OTJlYTg0ZDlhOTM5ZjUzYzMzODUxOTNhNGVmYTkyM2FiZDZkMThiYzQxYTY1MDgxMGY4ZWU5Y2YyODAiLCJhdWQiOiJ3c3M6Ly9yZWxheS53YWxsZXRjb25uZWN0LmNvbSIsImlhdCI6MTY3NTA4MjU5NCwiZXhwIjoxNjc1MDg2MTk0fQ.-51uws2Ur2IJzunuPQufXQvIhc6vXOUbsB3fZzfKX6eCsfRQGRoTr2T1ZnIH3A_Rmm1prtKMP7JgopW9xRvCCg";
    let addr = format!(
        "ws://localhost:8080?auth={}&projectId={}",
        token, project_id
    );

    let notification_json = serde_json::to_string(&cast_args.notification).unwrap();

    // encrypt
    let base64_notificaction =
        base64::engine::general_purpose::STANDARD_NO_PAD.encode(notification_json);

    let url = url::Url::parse(&addr).unwrap();
    let connection = tungstenite::connect(url).unwrap();
    let mut ws = connection.0;

    // Fetching accounts from db
    let accounts = cast_args
        .accounts
        .into_iter()
        .map(|x| x.0)
        .collect::<Vec<String>>();

    let mut cursor = db
        .collection::<ClientData>("clients")
        .find(
            doc! { "project_id":project_id, "id": {"$in": accounts}},
            None,
        )
        .await
        .unwrap();

    let mut push_messages = vec![];

    while let Some(data) = cursor.try_next().await.unwrap() {
        push_messages.push(
            serde_json::to_string(&JsonRpcPayload {
                id: "1".to_string(),
                jsonrpc: "2.0".to_string(),
                method: "irn_publish".to_string(),
                params: PublishParams {
                    topic: sha256::digest(data.sym_key).into(),
                    message: base64_notificaction.clone().into(),
                    ttl_secs: 8400,
                    tag: 4002,
                    prompt: false,
                },
            })
            .unwrap(),
        )
    }

    for msg in push_messages {
        ws.write_message(tungstenite::Message::Text(msg));
    }

    // let test = chacha20poly1305::ChaCha20Poly1305::generate_key(&mut OsRng);
    // let hex = hex::encode(test);
    // dbg!(&hex);
    // let test = hex::decode(hex).unwrap();
    // let cipher =
    // chacha20poly1305::ChaCha20Poly1305::new(GenericArray::from_slice(&test));

    // let cipher = chacha20poly1305::ChaCha20Poly1305::new(test);

    (
        StatusCode::OK,
        format!(
            "OK, {} v{}",
            state.build_info.crate_info.name, state.build_info.crate_info.version
        ),
    )
}
