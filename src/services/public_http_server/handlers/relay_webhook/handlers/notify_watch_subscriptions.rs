use {
    crate::{
        analytics::NotifyAnalytics,
        auth::{
            add_ttl, from_jwt, sign_jwt, verify_identity, AuthError, AuthorizedApp,
            NotifyServerSubscription, SharedClaims, SignJwtError,
            WatchSubscriptionsChangedRequestAuth, WatchSubscriptionsRequestAuth,
            WatchSubscriptionsResponseAuth,
        },
        error::NotifyServerError,
        metrics::Metrics,
        model::{
            helpers::{
                get_project_by_app_domain, get_subscription_watchers_for_account_by_app_or_all_app,
                get_subscriptions_by_account_and_maybe_app, upsert_subscription_watcher,
                SubscriberWithProject, SubscriptionWatcherQuery, UpsertSubscriptionWatcherError,
            },
            types::AccountId,
        },
        publish_relay_message::publish_relay_message,
        rate_limit::{self, Clock, RateLimitError},
        registry::storage::redis::Redis,
        rpc::{
            decode_key, derive_key, DecodeKeyError, JsonRpcRequest, JsonRpcResponse,
            JsonRpcResponseError, NotifySubscriptionsChanged, NotifyWatchSubscriptions,
            ResponseAuth,
        },
        services::public_http_server::handlers::relay_webhook::{
            error::{RelayMessageClientError, RelayMessageError, RelayMessageServerError},
            handlers::decrypt_message,
            RelayIncomingMessage,
        },
        spec::{
            NOTIFY_SUBSCRIPTIONS_CHANGED_ACT, NOTIFY_SUBSCRIPTIONS_CHANGED_METHOD,
            NOTIFY_SUBSCRIPTIONS_CHANGED_TAG, NOTIFY_SUBSCRIPTIONS_CHANGED_TTL,
            NOTIFY_WATCH_SUBSCRIPTIONS_ACT, NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_ACT,
            NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_TAG, NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_TTL,
        },
        state::AppState,
        types::{Envelope, EnvelopeType0, EnvelopeType1},
        utils::topic_from_key,
    },
    base64::Engine,
    chrono::{Duration, Utc},
    futures_util::StreamExt,
    relay_rpc::{auth::ed25519_dalek::SigningKey, domain::DecodedClientId, rpc::Publish},
    sqlx::PgPool,
    std::sync::Arc,
    thiserror::Error,
    tokio::sync::oneshot,
    tracing::{info, instrument},
    x25519_dalek::PublicKey,
};

pub const SUBSCRIPTION_WATCHER_LIMIT: i32 = 25;

#[instrument(name = "wc_notifyWatchSubscriptions", skip_all)]
pub async fn handle(msg: RelayIncomingMessage, state: &AppState) -> Result<(), RelayMessageError> {
    if msg.topic != state.notify_keys.key_agreement_topic {
        return Err(RelayMessageClientError::WrongNotifyWatchSubscriptionsTopic(
            msg.topic,
        ))?;
    }

    let envelope = Envelope::<EnvelopeType1>::from_bytes(
        base64::engine::general_purpose::STANDARD
            .decode(msg.message.to_string())
            .map_err(RelayMessageClientError::DecodeMessage)?,
    )
    .map_err(RelayMessageClientError::EnvelopeParse)?;

    let client_public_key = x25519_dalek::PublicKey::from(envelope.pubkey());

    if let Some(redis) = state.redis.as_ref() {
        notify_watch_subscriptions_rate_limit(redis, &client_public_key, &state.clock).await?;
    }

    let response_sym_key = derive_key(&client_public_key, &state.notify_keys.key_agreement_secret)
        .map_err(RelayMessageServerError::DeriveKey)?;
    let response_topic = topic_from_key(&response_sym_key);

    let req = decrypt_message::<NotifyWatchSubscriptions, _>(envelope, &response_sym_key)?;

    let (sdk_tx, mut sdk_rx) = oneshot::channel();
    async fn handle(
        state: &AppState,
        req: &JsonRpcRequest<NotifyWatchSubscriptions>,
        sdk_tx: oneshot::Sender<Option<Arc<str>>>,
        response_sym_key: &[u8; 32],
    ) -> Result<ResponseAuth, RelayMessageError> {
        info!("req.id: {}", req.id);
        info!("req.jsonrpc: {}", req.jsonrpc); // TODO verify this
        info!("req.method: {}", req.method); // TODO verify this

        let request_auth =
            from_jwt::<WatchSubscriptionsRequestAuth>(&req.params.watch_subscriptions_auth)
                .map_err(RelayMessageClientError::Jwt)?;
        info!(
            "request_auth.shared_claims.iss: {:?}",
            request_auth.shared_claims.iss
        );

        request_auth
            .validate()
            .map_err(RelayMessageServerError::NotifyServer)?; // TODO change to client error?

        sdk_tx
            .send(request_auth.sdk.map(Into::into))
            .map_err(|_| RelayMessageServerError::SdkOneshotSend)?;
        let request_iss_client_id =
            DecodedClientId::try_from_did_key(&request_auth.shared_claims.iss)
                .map_err(AuthError::JwtIssNotDidKey)
                .map_err(|e| RelayMessageServerError::NotifyServer(e.into()))?; // TODO change to client error?

        // Verify request
        let authorization = {
            if request_auth.shared_claims.act != NOTIFY_WATCH_SUBSCRIPTIONS_ACT {
                return Err(AuthError::InvalidAct)
                    .map_err(|e| RelayMessageServerError::NotifyServer(e.into()))?;
                // TODO change to client error?
            }

            verify_identity(
                &request_iss_client_id,
                &request_auth.ksu,
                &request_auth.sub,
                state.redis.as_ref(),
                state.provider.as_ref(),
                state.metrics.as_ref(),
            )
            .await?

            // TODO verify `sub_auth.aud` matches `notify-server.identity_keypair`

            // TODO merge code with deployment.rs#verify_jwt()
            //      - put desired `iss` value as an argument to make sure we verify
            //        it
        };
        let account = authorization.account;

        info!("authorization.app: {:?}", authorization.app);
        info!("request_auth.app: {:?}", request_auth.app);
        let app_domain = request_auth.app.map(|app| app.domain_arc());
        info!("app_domain: {app_domain:?}");
        check_app_authorization(&authorization.app, app_domain.as_deref())
            .map_err(RelayMessageClientError::AppNotAuthorized)?;

        let subscriptions = collect_subscriptions(
            account.clone(),
            app_domain.as_deref(),
            &state.postgres,
            state.metrics.as_ref(),
        )
        .await
        .map_err(RelayMessageServerError::CollectSubscriptions)?
        .iter()
        .map(|sub| NotifyServerSubscription {
            account: account.clone(),
            ..sub.clone()
        })
        .collect();

        let project = if let Some(app_domain) = app_domain {
            let project =
                get_project_by_app_domain(&app_domain, &state.postgres, state.metrics.as_ref())
                    .await
                    .map_err(|e| match e {
                        sqlx::Error::RowNotFound => RelayMessageError::Client(
                            RelayMessageClientError::NotifyWatchSubscriptionsAppDomainNotFound(
                                app_domain,
                            ),
                        ),
                        e => RelayMessageError::Server(RelayMessageServerError::NotifyServer(
                            e.into(),
                        )),
                    })?;
            Some(project.id)
        } else {
            None
        };
        info!("project: {project:?}");
        upsert_subscription_watcher(
            account,
            project,
            &request_auth.shared_claims.iss,
            &hex::encode(response_sym_key),
            Utc::now() + Duration::days(1),
            &state.postgres,
            state.metrics.as_ref(),
        )
        .await
        .map_err(|e| match e {
            UpsertSubscriptionWatcherError::LimitReached => {
                RelayMessageError::Client(RelayMessageClientError::SubscriptionWatcherLimitReached)
            }
            UpsertSubscriptionWatcherError::Sqlx(e) => RelayMessageError::Server(
                RelayMessageServerError::NotifyServer(NotifyServerError::Sqlx(e)),
            ),
        })?;

        let now = Utc::now();
        let response_message = WatchSubscriptionsResponseAuth {
            shared_claims: SharedClaims {
                iat: now.timestamp() as u64,
                exp: add_ttl(now, NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_TTL).timestamp() as u64,
                iss: state.notify_keys.authentication_client_id.to_did_key(),
                aud: request_iss_client_id.to_did_key(),
                act: NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_ACT.to_owned(),
                mjv: "1".to_owned(),
            },
            sub: request_auth.sub,
            sbs: subscriptions,
        };
        let response_auth = sign_jwt(response_message, &state.notify_keys.authentication_secret)
            .map_err(RelayMessageServerError::SignJwt)?;
        Ok(ResponseAuth { response_auth })
    }

    let result = handle(state, &req, sdk_tx, &response_sym_key).await;

    let response = match &result {
        Ok(result) => serde_json::to_vec(&JsonRpcResponse::new(req.id, result))
            .map_err(RelayMessageServerError::JsonRpcResponseSerialization)?,
        Err(e) => serde_json::to_vec(&JsonRpcResponseError::new(req.id, e.into()))
            .map_err(RelayMessageServerError::JsonRpcResponseErrorSerialization)?,
    };

    let sdk = match sdk_rx.try_recv() {
        Ok(sdk) => sdk,
        Err(oneshot::error::TryRecvError::Empty) => None,
        Err(oneshot::error::TryRecvError::Closed) => {
            Err(RelayMessageServerError::SdkOneshotReceive)?
        }
    };

    let envelope = Envelope::<EnvelopeType0>::new(&response_sym_key, response)
        .map_err(RelayMessageServerError::EnvelopeEncryption)?;
    let base64_notification = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    info!("Publishing response on topic {response_topic}");
    publish_relay_message(
        &state.relay_client,
        &Publish {
            topic: response_topic,
            message: base64_notification.into(),
            tag: NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_TAG,
            ttl_secs: NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_TTL.as_secs() as u32,
            prompt: false,
        },
        Some(Arc::new(msg)),
        sdk,
        state.metrics.as_ref(),
        &state.analytics,
    )
    .await
    .map_err(|e| RelayMessageServerError::NotifyServer(e.into()))?; // TODO change to client error?

    result.map(|_| ())
}

pub async fn notify_watch_subscriptions_rate_limit(
    redis: &Arc<Redis>,
    client_public_key: &PublicKey,
    clock: &Clock,
) -> Result<(), RateLimitError> {
    rate_limit::token_bucket(
        redis,
        format!(
            "notify-watch-subscriptions-{}",
            hex::encode(client_public_key.as_bytes())
        ),
        100,
        chrono::Duration::seconds(1),
        1,
        clock,
    )
    .await
}

#[derive(Debug, Error)]
pub enum CollectSubscriptionsError {
    #[error("Sqlx: {0}")]
    Sqlx(sqlx::Error),

    #[error("Decode key: {0}")]
    DecodeKey(DecodeKeyError),
}

#[instrument(skip(postgres, metrics))]
pub async fn collect_subscriptions(
    account: AccountId,
    app_domain: Option<&str>,
    postgres: &PgPool,
    metrics: Option<&Metrics>,
) -> Result<Vec<NotifyServerSubscription>, CollectSubscriptionsError> {
    info!("Called collect_subscriptions");

    let subscriptions = if let Some(app_domain) = app_domain {
        get_subscriptions_by_account_and_maybe_app(account, Some(app_domain), postgres, metrics)
            .await
            .map_err(CollectSubscriptionsError::Sqlx)?
    } else {
        get_subscriptions_by_account_and_maybe_app(account, None, postgres, metrics)
            .await
            .map_err(CollectSubscriptionsError::Sqlx)?
    };

    let subscriptions = {
        let try_subscriptions = subscriptions
            .into_iter()
            .map(|sub| {
                fn wrap(
                    sub: SubscriberWithProject,
                ) -> Result<NotifyServerSubscription, DecodeKeyError> {
                    Ok(NotifyServerSubscription {
                        app_domain: sub.app_domain,
                        app_authentication_key: DecodedClientId(decode_key(
                            &sub.authentication_public_key,
                        )?)
                        .to_did_key(),
                        sym_key: sub.sym_key,
                        account: sub.account,
                        scope: sub.scope,
                        expiry: sub.expiry.timestamp() as u64,
                        unread_notification_count: sub.unread_notification_count,
                    })
                }
                wrap(sub)
            })
            .collect::<Vec<_>>();
        let mut subscriptions = Vec::with_capacity(try_subscriptions.len());
        for result in try_subscriptions {
            let subscription = result.map_err(CollectSubscriptionsError::DecodeKey)?;
            subscriptions.push(subscription);
        }
        subscriptions
    };

    Ok(subscriptions)
}

#[derive(Debug, Error)]
pub enum PrepareSubscriptionWatchersError {
    #[error("Collect subscriptions: {0}")]
    CollectSubscriptions(CollectSubscriptionsError),

    #[error("Get subscription watchers for account by app or all app: {0}")]
    GetSubscriptionWatchersFoSrAccountByAppOrAllApp(sqlx::Error),
}

#[allow(clippy::type_complexity)]
#[instrument(skip_all, fields(account = %account, app_domain = %app_domain))]
pub async fn prepare_subscription_watchers(
    source_client_id: &DecodedClientId,
    mjv: &str,
    account: &AccountId,
    app_domain: &str,
    postgres: &PgPool,
    metrics: Option<&Metrics>,
) -> Result<
    (
        Vec<NotifyServerSubscription>,
        Vec<(SubscriptionWatcherQuery, Vec<NotifyServerSubscription>)>,
    ),
    PrepareSubscriptionWatchersError,
> {
    info!("Called prepare_subscription_watchers");

    // TODO can we combine collect_subscriptions() and get_subscription_watchers_for_account_by_app_or_all_app() queries?

    info!("Timing: Querying collect_subscriptions");
    let all_account_subscriptions = collect_subscriptions(account.clone(), None, postgres, metrics)
        .await
        .map_err(PrepareSubscriptionWatchersError::CollectSubscriptions)?;
    info!("Timing: Finished querying collect_subscriptions");

    let app_subscriptions = all_account_subscriptions
        .iter()
        .filter(|sub| sub.app_domain == app_domain)
        .cloned()
        .collect::<Vec<_>>();

    info!("Timing: Querying get_subscription_watchers_for_account_by_app_or_all_app");
    let subscription_watchers = get_subscription_watchers_for_account_by_app_or_all_app(
        account, app_domain, postgres, metrics,
    )
    .await
    .map_err(PrepareSubscriptionWatchersError::GetSubscriptionWatchersFoSrAccountByAppOrAllApp)?;
    info!("Timing: Finished querying get_subscription_watchers_for_account_by_app_or_all_app");

    let mut source_subscriptions = None;
    let mut watchers_with_subscriptions = Vec::with_capacity(subscription_watchers.len());

    let source_did_key = source_client_id.to_did_key();
    for watcher in subscription_watchers {
        let subscriptions = if watcher.project.is_some() {
            app_subscriptions.clone()
        } else {
            all_account_subscriptions.clone()
        };

        // mjv=0 for backwards compatibility: all watchers get all updates
        // mjv=1+ for new behavior: only watchers for the client that didn't perform the change get updates
        // https://github.com/WalletConnect/walletconnect-specs/pull/182
        let source_is_this_watcher = source_did_key == watcher.did_key;
        if source_is_this_watcher {
            assert!(
                source_subscriptions.is_none(),
                "Found multiple subscription watchers for same did_key: {}",
                watcher.did_key
            );
            source_subscriptions = Some(
                subscriptions
                    .iter()
                    .map(|sub| NotifyServerSubscription {
                        account: account.clone(),
                        ..sub.clone()
                    })
                    .collect(),
            );
        }

        if !source_is_this_watcher || mjv == "0" {
            let subscriptions = subscriptions
                .iter()
                .map(|sub| NotifyServerSubscription {
                    account: watcher.account.clone(),
                    ..sub.clone()
                })
                .collect();
            watchers_with_subscriptions.push((watcher, subscriptions));
        }
    }

    // In-case the source client never called watchSubscriptions, we can still give back a response
    let source_subscriptions = source_subscriptions.unwrap_or(app_subscriptions);

    Ok((source_subscriptions, watchers_with_subscriptions))
}

#[allow(clippy::too_many_arguments)]
#[instrument(skip_all)]
pub async fn send_to_subscription_watchers(
    watchers_with_subscriptions: Vec<(SubscriptionWatcherQuery, Vec<NotifyServerSubscription>)>,
    authentication_secret: &SigningKey,
    authentication_client_id: &DecodedClientId,
    http_client: &relay_client::http::Client,
    relay_request: Arc<RelayIncomingMessage>,
    sdk: Option<Arc<str>>,
    metrics: Option<&Metrics>,
    analytics: &NotifyAnalytics,
) -> Result<(), SubscriptionWatcherSendError> {
    let results = futures_util::stream::iter(watchers_with_subscriptions)
        .map(|(watcher, subscriptions)| {
            let relay_request = relay_request.clone();
            let sdk = sdk.clone();
            async move {
                info!(
                    "Timing: Sending watchSubscriptionsChanged to watcher.did_key: {}",
                    watcher.did_key
                );
                send(
                    subscriptions,
                    &watcher.account,
                    watcher.did_key.clone(),
                    &watcher.sym_key,
                    authentication_secret,
                    authentication_client_id,
                    http_client,
                    relay_request,
                    sdk,
                    metrics,
                    analytics,
                )
                .await?;
                info!(
                    "Timing: Sent watchSubscriptionsChanged to watcher.did_key: {}",
                    watcher.did_key
                );
                Ok(())
            }
        })
        .buffer_unordered(10)
        .collect::<Vec<Result<(), SubscriptionWatcherSendError>>>()
        .await;
    for result in results {
        result?;
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum SubscriptionWatcherSendError {
    #[error("Decode key: {0}")]
    DecodeKey(DecodeKeyError),

    #[error("Sign JWT: {0}")]
    SignJwt(#[from] SignJwtError),

    #[error("Envelope serialization: {0}")]
    EnvelopeSerialization(#[from] serde_json::error::Error),

    #[error("Envelope encryption: {0}")]
    EnvelopeEncryption(chacha20poly1305::aead::Error),

    #[error("Relay publish: {0}")]
    RelayPublish(relay_client::error::Error<relay_rpc::rpc::PublishError>),
}

#[allow(clippy::too_many_arguments)]
#[instrument(skip_all, fields(account = %account, aud = %aud, subscriptions_count = %subscriptions.len()))]
async fn send(
    subscriptions: Vec<NotifyServerSubscription>,
    account: &AccountId,
    aud: String,
    sym_key: &str,
    authentication_secret: &SigningKey,
    authentication_client_id: &DecodedClientId,
    http_client: &relay_client::http::Client,
    relay_request: Arc<RelayIncomingMessage>,
    sdk: Option<Arc<str>>,
    metrics: Option<&Metrics>,
    analytics: &NotifyAnalytics,
) -> Result<(), SubscriptionWatcherSendError> {
    let now = Utc::now();
    let response_message = WatchSubscriptionsChangedRequestAuth {
        shared_claims: SharedClaims {
            iat: now.timestamp() as u64,
            exp: add_ttl(now, NOTIFY_SUBSCRIPTIONS_CHANGED_TTL).timestamp() as u64,
            iss: authentication_client_id.to_did_key(),
            aud,
            act: NOTIFY_SUBSCRIPTIONS_CHANGED_ACT.to_owned(),
            mjv: "1".to_owned(),
        },
        sub: account.to_did_pkh(),
        sbs: subscriptions,
    };
    let auth = sign_jwt(response_message, authentication_secret)
        .map_err(SubscriptionWatcherSendError::SignJwt)?;
    let request = JsonRpcRequest::new(
        NOTIFY_SUBSCRIPTIONS_CHANGED_METHOD,
        NotifySubscriptionsChanged {
            subscriptions_changed_auth: auth,
        },
    );

    let sym_key = decode_key(sym_key).map_err(SubscriptionWatcherSendError::DecodeKey)?;
    let envelope = Envelope::<EnvelopeType0>::new(
        &sym_key,
        serde_json::to_vec(&request)
            .map_err(SubscriptionWatcherSendError::EnvelopeSerialization)?,
    )
    .map_err(SubscriptionWatcherSendError::EnvelopeEncryption)?;
    let base64_notification = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    let topic = topic_from_key(&sym_key);
    publish_relay_message(
        http_client,
        &Publish {
            topic,
            message: base64_notification.into(),
            tag: NOTIFY_SUBSCRIPTIONS_CHANGED_TAG,
            ttl_secs: NOTIFY_SUBSCRIPTIONS_CHANGED_TTL.as_secs() as u32,
            prompt: false,
        },
        Some(relay_request),
        sdk,
        metrics,
        analytics,
    )
    .await
    .map_err(SubscriptionWatcherSendError::RelayPublish)?;

    Ok(())
}

#[derive(Debug, PartialEq, thiserror::Error)]
pub enum CheckAppAuthorizationError {
    #[error("Requested app {requested:?} is not authorized for {authorized}")]
    AppNotAuthorized {
        requested: Option<String>,
        authorized: String,
    },
}

fn check_app_authorization(
    authorized_app: &AuthorizedApp,
    app_domain: Option<&str>,
) -> Result<(), CheckAppAuthorizationError> {
    if let AuthorizedApp::Limited(authorized) = authorized_app {
        let Some(requested) = app_domain else {
            return Err(CheckAppAuthorizationError::AppNotAuthorized {
                // app_domain is always None here, meaning they are trying to watch all apps, which
                // is not authorized
                requested: None,
                authorized: authorized.to_owned(),
            });
        };
        if authorized != requested {
            return Err(CheckAppAuthorizationError::AppNotAuthorized {
                requested: Some(requested.to_owned()),
                authorized: authorized.to_owned(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_check_app_authorization() {
        assert_eq!(
            check_app_authorization(&AuthorizedApp::Unlimited, None),
            Ok(())
        );
        assert_eq!(
            check_app_authorization(&AuthorizedApp::Unlimited, Some("app.example.com")),
            Ok(())
        );
        assert_eq!(
            check_app_authorization(
                &AuthorizedApp::Limited("app.example.com".to_owned()),
                Some("app.example.com")
            ),
            Ok(())
        );
        assert_eq!(
            check_app_authorization(
                &AuthorizedApp::Limited("app.example.com".to_owned()),
                Some("example.com")
            ),
            Err(CheckAppAuthorizationError::AppNotAuthorized {
                requested: Some("example.com".to_owned()),
                authorized: "app.example.com".to_owned(),
            })
        );
        assert_eq!(
            check_app_authorization(&AuthorizedApp::Limited("app.example.com".to_owned()), None),
            Err(CheckAppAuthorizationError::AppNotAuthorized {
                requested: None,
                authorized: "app.example.com".to_owned(),
            })
        );
    }
}
