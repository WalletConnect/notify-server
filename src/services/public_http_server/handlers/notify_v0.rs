use {
    crate::{
        analytics::subscriber_notification::SubscriberNotificationParams,
        error,
        jsonrpc::{JsonRpcParams, JsonRpcPayload, NotifyPayload},
        model::{
            helpers::{
                get_project_by_project_id, get_subscribers_for_project_in, SubscriberWithScope,
            },
            types::AccountId,
        },
        notify_message::{sign_message, ProjectSigningDetails},
        registry::extractor::AuthedProjectId,
        services::websocket_server::decode_key,
        spec::{NOTIFY_MESSAGE_TAG, NOTIFY_MESSAGE_TTL},
        state::AppState,
        types::{Envelope, EnvelopeType0, Notification},
    },
    axum::{extract::State, http::StatusCode, response::IntoResponse, Json},
    base64::Engine,
    error::Result,
    futures::FutureExt,
    relay_rpc::{
        domain::{ClientId, DecodedClientId, ProjectId, Topic},
        rpc::{msg_id::MsgId, Publish},
    },
    serde::{Deserialize, Serialize},
    std::{collections::HashSet, sync::Arc, time::Duration},
    tokio::time::error::Elapsed,
    tracing::{info, warn},
    uuid::Uuid,
    wc::metrics::otel::{Context, KeyValue},
};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NotifyBody {
    pub notification: Notification,
    pub accounts: Vec<AccountId>,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
pub struct SendFailure {
    pub account: AccountId,
    pub reason: String,
}

#[derive(Clone)]
struct PublishJob {
    client_pk: Uuid,
    account: AccountId,
    topic: Topic,
    message: String,
}

// Change String to Account
// Change String to Error
#[derive(Serialize, Deserialize, Debug)]
pub struct Response {
    pub sent: HashSet<AccountId>,
    pub failed: HashSet<SendFailure>,
    pub not_found: HashSet<AccountId>,
}

pub async fn handler(
    State(state): State<Arc<AppState>>,
    AuthedProjectId(project_id, _): AuthedProjectId,
    Json(notify_args): Json<NotifyBody>,
) -> Result<axum::response::Response> {
    // Request id for logs
    let request_id = uuid::Uuid::new_v4();
    let timer = std::time::Instant::now();

    // TODO handle project not found
    let project = get_project_by_project_id(project_id.clone(), &state.postgres).await?;
    let project_signing_details = {
        let private_key = ed25519_dalek::SigningKey::from_bytes(&decode_key(
            &project.authentication_private_key,
        )?);
        let decoded_client_id = DecodedClientId(decode_key(&project.authentication_public_key)?);
        let identity = ClientId::from(decoded_client_id);
        ProjectSigningDetails {
            identity,
            private_key,
            app: project.app_domain.into(),
        }
    };

    let mut response = Response {
        sent: HashSet::new(),
        failed: HashSet::new(),
        not_found: HashSet::new(),
    };

    let NotifyBody {
        notification,
        accounts,
    } = notify_args;
    let notification_type = notification.r#type.clone().into();

    // We assume all accounts were not found untill found
    response.not_found.extend(accounts.iter().cloned());

    let subscribers =
        get_subscribers_for_project_in(project.id, &accounts, &state.postgres).await?;

    // Generate publish jobs - this will also remove accounts from not_found
    // Prepares the encrypted message and gets the topic for each account
    let jobs = generate_publish_jobs(
        notification,
        subscribers,
        &mut response,
        &project_signing_details,
    )
    .await?;

    // Attempts to send to all found accounts, waiting for relay ack for
    // NOTIFY_TIMEOUT seconds
    process_publish_jobs(
        jobs,
        notification_type,
        state.relay_http_client.clone(),
        &mut response,
        request_id,
        &state,
        project.id,
        project_id.clone(),
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

#[allow(clippy::too_many_arguments)]
async fn process_publish_jobs(
    jobs: Vec<PublishJob>,
    notification_type: Arc<str>,
    client: Arc<relay_client::http::Client>,
    response: &mut Response,
    request_id: Uuid,
    state: &Arc<AppState>,
    project_pk: Uuid,
    project_id: ProjectId,
) -> Result<()> {
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
            account: AccountId,
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
                let project_id = project_id.clone();
                let notification_type = notification_type.clone();
                move |result| {
                    if result.is_ok() {
                        state.analytics.message(SubscriberNotificationParams {
                            project_pk,
                            project_id,
                            subscriber_pk: job.client_pk,
                            account: job.account,
                            notification_type,
                            notify_topic: job.topic,
                            message_id: msg_id.into(),
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
                info!(
                    "[{request_id}] Successfully sent notification to {account} on topic: {topic}",
                );
                response.sent.insert(account);
            }
            Err((error, account, topic)) => {
                warn!(
                    "[{request_id}] Error sending notification to account {account} on topic: \
                     {topic}: {error:?}"
                );
                response.failed.insert(SendFailure {
                    account,
                    reason: "Internal error".into(),
                });
            }
        }
    }

    Ok(())
}

async fn generate_publish_jobs(
    notification: Notification,
    subscribers: Vec<SubscriberWithScope>,
    response: &mut Response,
    project_signing_details: &ProjectSigningDetails,
) -> Result<Vec<PublishJob>> {
    let mut jobs = vec![];

    let id = chrono::Utc::now().timestamp_millis().unsigned_abs();

    let notification = Arc::new(notification);
    for subscriber in subscribers {
        response.not_found.remove(&subscriber.account);

        if !subscriber.scope.contains(&notification.r#type) {
            response.failed.insert(SendFailure {
                account: subscriber.account.clone(),
                reason: "Client is not subscribed to this notification type".into(),
            });
            continue;
        }

        let message = JsonRpcPayload {
            id,
            jsonrpc: "2.0".to_string(),
            params: JsonRpcParams::Push(NotifyPayload {
                message_auth: sign_message(
                    notification.clone(),
                    subscriber.account.clone(),
                    project_signing_details,
                )?
                .to_string(),
            }),
        };

        let sym_key = decode_key(&subscriber.sym_key)?;

        let envelope = Envelope::<EnvelopeType0>::new(&sym_key, &message)?;

        let base64_notification =
            base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

        let topic = Topic::new(sha256::digest(&sym_key).into());

        jobs.push(PublishJob {
            client_pk: subscriber.id,
            account: subscriber.account,
            topic,
            message: base64_notification,
        })
    }
    Ok(jobs)
}

fn send_metrics(metrics: &crate::metrics::Metrics, response: &Response, timer: std::time::Instant) {
    let ctx = Context::current();
    metrics.dispatched_notifications.add(
        &ctx,
        response.sent.len() as u64,
        &[KeyValue::new("type", "sent")],
    );

    metrics.dispatched_notifications.add(
        &ctx,
        response.failed.len() as u64,
        &[KeyValue::new("type", "failed")],
    );

    metrics.dispatched_notifications.add(
        &ctx,
        response.not_found.len() as u64,
        &[KeyValue::new("type", "not_found")],
    );

    metrics
        .notify_latency
        .record(&ctx, timer.elapsed().as_millis().try_into().unwrap(), &[])
}
