use {
    crate::{
        auth::jwt_token,
        error,
        jsonrpc::{JsonRpcParams, JsonRpcPayload, PublishParams},
        state::AppState,
        types::{ClientData, Envelope, EnvelopeType0, Notification},
    },
    axum::{
        extract::{ConnectInfo, Path, State},
        http::StatusCode,
        response::IntoResponse,
        Json,
    },
    base64::Engine,
    log::warn,
    mongodb::bson::doc,
    opentelemetry::{Context, KeyValue},
    serde::{Deserialize, Serialize},
    std::{
        collections::{HashMap, HashSet},
        net::SocketAddr,
        sync::Arc,
    },
    tokio_stream::StreamExt,
    tracing::info,
};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NotifyBody {
    pub notification: Notification,
    pub accounts: Vec<String>,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
pub struct SendFailure {
    pub account: String,
    pub reason: String,
}

// Change String to Account
// Change String to Error
#[derive(Serialize, Deserialize, Debug)]
pub struct Response {
    pub sent: HashSet<String>,
    pub failed: HashSet<SendFailure>,
    pub not_found: HashSet<String>,
}

pub async fn handler(
    ConnectInfo(_addr): ConnectInfo<SocketAddr>,
    Path(project_id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(cast_args): Json<NotifyBody>,
) -> Result<axum::response::Response, error::Error> {
    let timer = std::time::Instant::now();
    let db = state.database.clone();
    let NotifyBody {
        notification,
        accounts,
    } = cast_args;
    let mut confirmed_sends = HashSet::new();
    let mut failed_sends: HashSet<SendFailure> = HashSet::new();

    let id = chrono::Utc::now().timestamp_millis().unsigned_abs();

    let mut cursor = db
        .collection::<ClientData>(&project_id)
        .find(
            doc! { "_id": {"$in": &accounts}, "scope": { "$elemMatch": { "$eq": &notification.r#type } }},
            None,
        )
        .await?;

    let mut not_found: HashSet<String> = accounts.into_iter().collect();

    let mut clients = HashMap::<String, Vec<(String, String)>>::new();

    let message = &JsonRpcPayload {
        id,
        jsonrpc: "2.0".to_string(),
        params: JsonRpcParams::Push(notification.clone()),
    };

    while let Some(data) = cursor.try_next().await.unwrap() {
        not_found.remove(&data.id);

        let envelope = Envelope::<EnvelopeType0>::new(&data.sym_key, message)?;

        let base64_notification =
            base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

        let id = chrono::Utc::now().timestamp_millis().unsigned_abs();

        let message = serde_json::to_string(&JsonRpcPayload {
            id,
            jsonrpc: "2.0".to_string(),
            params: JsonRpcParams::Publish(PublishParams {
                topic: sha256::digest(&*hex::decode(data.sym_key)?),
                message: base64_notification.clone(),
                ttl_secs: 86400,
                tag: 4002,
                prompt: true,
            }),
        })?;

        clients
            .entry(data.relay_url)
            .or_default()
            .push((message, data.id));
    }

    for (url, notifications) in clients {
        let token = jwt_token(&url, &state.keypair)?;
        let relay_query = format!("projectId={project_id}&auth={token}");

        let mut url = url::Url::parse(&url)?;
        url.set_query(Some(&relay_query));
        let mut connection = tungstenite::connect(&url);

        for notification_data in notifications {
            let (encrypted_notification, sender) = notification_data;

            match &mut connection {
                Ok(connection) => {
                    let ws = &mut connection.0;
                    match ws.write_message(tungstenite::Message::Text(encrypted_notification)) {
                        Ok(_) => {
                            info!("Casting to client");
                            confirmed_sends.insert(sender);
                        }
                        Err(e) => {
                            failed_sends.insert(SendFailure {
                                account: sender,
                                reason: e.to_string(),
                            });
                        }
                    };
                }
                Err(e) => {
                    warn!("{}", e);
                    failed_sends.insert(SendFailure {
                        account: sender,
                        reason: format!(
                            "Failed connecting to {}://{}",
                            &url.scheme(),
                            // Safe unwrap since all stored urls are "wss://", for which host
                            // always exists
                            &url.host().unwrap()
                        ),
                    });
                }
            }
        }
    }

    if let Some(metrics) = &state.metrics {
        let ctx = Context::current();
        metrics
            .dispatched_notifications
            .add(&ctx, confirmed_sends.len() as u64, &[
                KeyValue::new("type", "sent"),
                KeyValue::new("project_id", project_id.clone()),
            ]);

        metrics
            .dispatched_notifications
            .add(&ctx, failed_sends.len() as u64, &[
                KeyValue::new("type", "failed"),
                KeyValue::new("project_id", project_id.clone()),
            ]);

        metrics
            .dispatched_notifications
            .add(&ctx, not_found.len() as u64, &[
                KeyValue::new("type", "not_found"),
                KeyValue::new("project_id", project_id.clone()),
            ]);

        metrics
            .send_latency
            .record(&ctx, timer.elapsed().as_millis().try_into().unwrap(), &[
                KeyValue::new("project_id", project_id.clone()),
            ])
    }

    // Get them into one struct and serialize as json
    let response = Response {
        sent: confirmed_sends,
        failed: failed_sends,
        not_found,
    };

    info!(
        "Response: {:?} for notify from project: {}",
        response, project_id
    );

    Ok((StatusCode::OK, Json(response)).into_response())
}
