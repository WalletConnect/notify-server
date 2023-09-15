use {
    super::subscribe_topic::ProjectData,
    crate::{
        analytics::message_info::MessageInfo,
        auth::add_ttl,
        error,
        extractors::AuthedProjectId,
        jsonrpc::{JsonRpcParams, JsonRpcPayload, NotifyPayload},
        spec::{NOTIFY_MESSAGE_TAG, NOTIFY_MESSAGE_TTL},
        state::AppState,
        types::{ClientData, Envelope, EnvelopeType0, Notification},
        websocket_service::decode_key,
    },
    axum::{
        extract::{ConnectInfo, State},
        http::StatusCode,
        response::IntoResponse,
        Json,
    },
    base64::Engine,
    chrono::Utc,
    ed25519_dalek::Signer,
    error::Result,
    futures::FutureExt,
    mongodb::bson::doc,
    relay_rpc::{
        domain::{ClientId, DecodedClientId, Topic},
        jwt::{JwtHeader, JWT_HEADER_ALG, JWT_HEADER_TYP},
        rpc::{msg_id::MsgId, Publish},
    },
    serde::{Deserialize, Serialize},
    std::{collections::HashSet, net::SocketAddr, sync::Arc, time::Duration},
    tokio::time::error::Elapsed,
    tokio_stream::StreamExt,
    tracing::{info, warn},
    wc::metrics::otel::{Context, KeyValue},
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

#[derive(Clone)]
struct PublishJob {
    account: String,
    topic: Topic,
    message: String,
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
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    AuthedProjectId(project_id, _): AuthedProjectId,
    Json(notify_args): Json<NotifyBody>,
) -> Result<axum::response::Response> {
    // Request id for logs
    let request_id = uuid::Uuid::new_v4();
    let timer = std::time::Instant::now();

    let mut response = Response {
        sent: HashSet::new(),
        failed: HashSet::new(),
        not_found: HashSet::new(),
    };

    let NotifyBody {
        notification,
        accounts,
    } = notify_args;

    // We assume all accounts were not found untill found
    accounts.iter().for_each(|account| {
        response.not_found.insert(account.clone());
    });

    // Get the accounts
    let cursor = state
        .database
        .collection::<ClientData>(&project_id)
        .find(doc! { "_id": {"$in": &accounts}}, None)
        .await?;

    let project_data: ProjectData = state
        .database
        .collection::<ProjectData>("project_data")
        .find_one(doc! { "_id": project_id.clone()}, None)
        .await?
        .ok_or(error::Error::NoProjectDataForTopic(project_id.clone()))?;

    // Generate publish jobs - this will also remove accounts from not_found
    // Prepares the encrypted message and gets the topic for each account
    let jobs = generate_publish_jobs(notification, cursor, &mut response, &project_data).await?;

    // Attempts to send to all found accounts, waiting for relay ack for
    // NOTIFY_TIMEOUT seconds
    process_publish_jobs(
        jobs,
        state.http_relay_client.clone(),
        &mut response,
        request_id,
        &addr,
        &state,
        &project_id,
    )
    .await?;

    info!("[{request_id}] Response: {response:?} for notify from project: {project_id}");

    if let Some(metrics) = &state.metrics {
        send_metrics(metrics, &response, timer);
    }

    Ok((StatusCode::OK, Json(response)).into_response())
}

const NOTIFY_TIMEOUT: u64 = 45;

#[derive(Debug)]
enum JobError {
    Error(relay_client::error::Error),
    Elapsed(Elapsed),
}

async fn process_publish_jobs(
    jobs: Vec<PublishJob>,
    client: Arc<relay_client::http::Client>,
    response: &mut Response,
    request_id: uuid::Uuid,
    addr: &SocketAddr,
    state: &Arc<AppState>,
    project_id: &str,
) -> Result<()> {
    let geo_ip =
        state
            .analytics
            .geoip
            .lookup_geo_data(addr.ip())
            .map_or((None, None, None), |geo| {
                (
                    geo.country,
                    geo.continent,
                    geo.region.map(|r| Arc::from(r.join(", "))),
                )
            });

    let timer = std::time::Instant::now();
    let futures = jobs.into_iter().map(|job| {
        let remaining_time = timer.elapsed();
        let timeout_duration = Duration::from_secs(NOTIFY_TIMEOUT) - remaining_time;

        let publish = Publish {
            topic: job.topic.clone(),
            message: job.message.clone().into(),
            ttl_secs: NOTIFY_MESSAGE_TTL.as_secs() as u32,
            tag: NOTIFY_MESSAGE_TAG,
            prompt: true,
        };

        let msg_id = publish.msg_id();
        info!(
            "[{request_id}] Sending notification for {account} on topic: {topic} with {msg_id}",
            topic = job.topic,
            account = job.account,
            msg_id = msg_id
        );

        async fn do_publish(
            client: Arc<relay_client::http::Client>,
            account: String,
            publish: Publish,
        ) -> std::result::Result<(), relay_client::error::Error> {
            let go = || {
                client.publish(
                    // Careful: only read from `Publish` object to ensure proper msg_id above
                    publish.topic.clone(),
                    publish.message.clone(),
                    publish.tag,
                    Duration::from_secs(publish.ttl_secs as u64),
                    publish.prompt,
                )
            };

            let mut tries = 0;
            while let Err(e) = go().await {
                warn!(
                    "Temporary error publishing notification for account {} on topic {}, retrying \
                     in 1s: {e:?}",
                    account, publish.topic
                );
                tries += 1;
                if tries >= 10 {
                    return Err(e);
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            if tries > 0 {
                warn!(
                    "Publishing to account {} on topic {} took {} tries",
                    account, publish.topic, tries
                );
            }
            Ok(())
        }

        let task = do_publish(client.clone(), job.account.clone(), publish);
        let result = tokio::time::timeout(timeout_duration, task);
        result
            .map({
                let job = job.clone();
                move |result| match result {
                    Ok(Ok(())) => Ok((job.account, job.topic)),
                    Ok(Err(e)) => Err((JobError::Error(e), job.account, job.topic)),
                    Err(e) => Err((JobError::Elapsed(e), job.account, job.topic)),
                }
            })
            .map({
                let (country, continent, region) = geo_ip.clone();
                move |result| {
                    if result.is_ok() {
                        state.analytics.message(MessageInfo {
                            region,
                            country,
                            continent,
                            project_id: project_id.into(),
                            msg_id: msg_id.into(),
                            topic: job.topic.to_string().into(),
                            account: job.account.clone().into(),
                            sent_at: gorgon::time::now(),
                        });
                    }
                    result
                }
            })
    });

    let results = futures::future::join_all(futures).await;

    for result in results {
        match result {
            Ok((account, topic)) => {
                response.sent.insert(account.to_string());
                info!(
                    "[{request_id}] Successfully sent notification to {account} on topic: {topic}",
                );
            }
            Err((error, account, topic)) => {
                warn!(
                    "[{request_id}] Error sending notification to account {account} on topic: \
                     {topic}: {error:?}"
                );
                response.failed.insert(SendFailure {
                    account: account.to_string(),
                    reason: "Internal error".into(),
                });
            }
        }
    }

    Ok(())
}

async fn generate_publish_jobs(
    notification: Notification,
    mut cursor: mongodb::Cursor<ClientData>,
    response: &mut Response,
    project_data: &ProjectData,
) -> Result<Vec<PublishJob>> {
    let mut jobs = vec![];

    let id = chrono::Utc::now().timestamp_millis().unsigned_abs();

    while let Some(client_data) = cursor.try_next().await? {
        response.not_found.remove(&client_data.id);

        if !client_data.scope.contains(&notification.r#type) {
            response.failed.insert(SendFailure {
                account: client_data.id.clone(),
                reason: "Client is not subscribed to this notification type".into(),
            });
            continue;
        }

        let message = JsonRpcPayload {
            id,
            jsonrpc: "2.0".to_string(),
            params: JsonRpcParams::Push(NotifyPayload {
                message_auth: sign_message(&notification, project_data, &client_data)?.to_string(),
            }),
        };

        let sym_key = decode_key(&client_data.sym_key)?;

        let envelope = Envelope::<EnvelopeType0>::new(&sym_key, &message)?;

        let base64_notification =
            base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

        let topic = Topic::new(sha256::digest(&sym_key).into());

        jobs.push(PublishJob {
            topic,
            message: base64_notification,
            account: client_data.id,
        })
    }
    Ok(jobs)
}

fn send_metrics(metrics: &crate::metrics::Metrics, response: &Response, timer: std::time::Instant) {
    let ctx = Context::current();
    metrics
        .dispatched_notifications
        .add(&ctx, response.sent.len() as u64, &[KeyValue::new(
            "type", "sent",
        )]);

    metrics
        .dispatched_notifications
        .add(&ctx, response.failed.len() as u64, &[KeyValue::new(
            "type", "failed",
        )]);

    metrics
        .dispatched_notifications
        .add(&ctx, response.not_found.len() as u64, &[KeyValue::new(
            "type",
            "not_found",
        )]);

    metrics
        .notify_latency
        .record(&ctx, timer.elapsed().as_millis().try_into().unwrap(), &[])
}

fn sign_message(
    msg: &Notification,
    project_data: &ProjectData,
    client_data: &ClientData,
) -> Result<String> {
    let decoded_client_id = DecodedClientId(
        hex::decode(project_data.identity_keypair.public_key.clone())?[0..32].try_into()?,
    );
    let identity = ClientId::from(decoded_client_id).to_string();

    let did_pkh = format!("did:pkh:{}", client_data.id);

    let now = Utc::now();
    let message = {
        let msg = JwtMessage {
            iat: now.timestamp(),
            exp: add_ttl(now, NOTIFY_MESSAGE_TTL).timestamp(),
            iss: format!("did:key:{identity}"),
            act: "notify_message".to_string(),
            sub: did_pkh,
            app: project_data.app_domain.to_string(),
            msg: msg.clone(),
        };
        let serialized = serde_json::to_string(&msg)?;
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serialized)
    };

    let header = {
        let data = JwtHeader {
            typ: JWT_HEADER_TYP,
            alg: JWT_HEADER_ALG,
        };

        let serialized = serde_json::to_string(&data)?;

        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serialized)
    };

    let private_key = ed25519_dalek::SigningKey::from_bytes(&decode_key(
        &project_data.identity_keypair.private_key,
    )?);

    let message = format!("{header}.{message}");
    let signature = private_key.sign(message.as_bytes());
    let signature = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature.to_bytes());

    Ok(format!("{message}.{signature}"))
}

#[derive(Serialize, Deserialize, Debug)]
pub struct JwtMessage {
    pub iat: i64, // issued at
    pub exp: i64, // expiry
    // TODO: This was changed from notify pubkey, should be confirmed if we want to keep this
    pub iss: String,       // dapps identity key
    pub act: String,       // action intent (must be "notify_message")
    pub sub: String,       // did:pkh of blockchain account
    pub app: String,       // dapp domain url
    pub msg: Notification, // message
}
