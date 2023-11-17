use {
    crate::{
        auth::{
            add_ttl, from_jwt, sign_jwt, verify_identity, AuthError, AuthorizedApp,
            NotifyServerSubscription, SharedClaims, WatchSubscriptionsChangedRequestAuth,
            WatchSubscriptionsRequestAuth, WatchSubscriptionsResponseAuth,
        },
        error::Error,
        metrics::Metrics,
        model::{
            helpers::{
                get_project_by_app_domain, get_subscription_watchers_for_account_by_app_or_all_app,
                get_subscriptions_by_account, get_subscriptions_by_account_and_app,
                upsert_subscription_watcher, SubscriberWithProject,
            },
            types::AccountId,
        },
        publish_relay_message::publish_relay_message,
        services::websocket_server::{
            decode_key, derive_key, handlers::decrypt_message, NotifyRequest, NotifyResponse,
            NotifyWatchSubscriptions,
        },
        spec::{
            NOTIFY_SUBSCRIPTIONS_CHANGED_METHOD, NOTIFY_SUBSCRIPTIONS_CHANGED_TAG,
            NOTIFY_SUBSCRIPTIONS_CHANGED_TTL, NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_TAG,
            NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_TTL,
        },
        state::AppState,
        types::{Envelope, EnvelopeType0, EnvelopeType1},
        Result,
    },
    base64::Engine,
    chrono::{Duration, Utc},
    relay_client::websocket::PublishedMessage,
    relay_rpc::{
        domain::{DecodedClientId, Topic},
        rpc::{Publish, JSON_RPC_VERSION_STR},
    },
    serde_json::{json, Value},
    sqlx::PgPool,
    tracing::{info, instrument},
};

#[instrument(name = "wc_notifyWatchSubscriptions", skip_all)]
pub async fn handle(msg: PublishedMessage, state: &AppState) -> Result<()> {
    if msg.topic != state.notify_keys.key_agreement_topic {
        return Err(Error::WrongNotifyWatchSubscriptionsTopic(msg.topic));
    }

    let envelope = Envelope::<EnvelopeType1>::from_bytes(
        base64::engine::general_purpose::STANDARD.decode(msg.message.to_string())?,
    )?;

    let client_public_key = x25519_dalek::PublicKey::from(envelope.pubkey());
    info!("client_public_key: {client_public_key:?}");
    let response_sym_key = derive_key(&client_public_key, &state.notify_keys.key_agreement_secret)?;
    let response_topic = sha256::digest(&response_sym_key);

    let msg: NotifyRequest<NotifyWatchSubscriptions> =
        decrypt_message(envelope, &response_sym_key)?;

    let id = msg.id;

    let request_auth =
        from_jwt::<WatchSubscriptionsRequestAuth>(&msg.params.watch_subscriptions_auth)?;
    info!(
        "request_auth.shared_claims.iss: {:?}",
        request_auth.shared_claims.iss
    );

    // Verify request
    let authorization = {
        if request_auth.shared_claims.act != "notify_watch_subscriptions" {
            return Err(AuthError::InvalidAct)?;
        }

        verify_identity(
            &request_auth.shared_claims.iss,
            &request_auth.ksu,
            &request_auth.sub,
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
    let app_domain = request_auth
        .app
        .map(|app| {
            app.strip_prefix("did:web:")
                .map(str::to_owned)
                .ok_or(Error::AppNotDidWeb)
        })
        .transpose()?;
    info!("app_domain: {app_domain:?}");
    check_app_authorization(&authorization.app, app_domain.as_deref())?;

    let subscriptions = collect_subscriptions(
        account.clone(),
        app_domain.as_deref(),
        &state.postgres,
        state.metrics.as_ref(),
    )
    .await?;

    let project = if let Some(app_domain) = app_domain {
        let project =
            get_project_by_app_domain(&app_domain, &state.postgres, state.metrics.as_ref())
                .await
                .map_err(|e| match e {
                    sqlx::Error::RowNotFound => Error::NoProjectDataForAppDomain(app_domain),
                    e => e.into(),
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
    .await?;

    // Respond
    {
        let identity: DecodedClientId =
            DecodedClientId(state.notify_keys.authentication_public.to_bytes());

        let now = Utc::now();
        let response_message = WatchSubscriptionsResponseAuth {
            shared_claims: SharedClaims {
                iat: now.timestamp() as u64,
                exp: add_ttl(now, NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_TTL).timestamp() as u64,
                iss: format!("did:key:{identity}"),
                act: "notify_watch_subscriptions_response".to_string(),
                aud: request_auth.shared_claims.iss,
            },
            sub: request_auth.sub,
            sbs: subscriptions,
        };
        let response_auth = sign_jwt(response_message, &state.notify_keys.authentication_secret)?;
        let response = NotifyResponse::<Value> {
            id,
            jsonrpc: JSON_RPC_VERSION_STR.to_owned(),
            result: json!({ "responseAuth": response_auth }), // TODO use structure
        };

        let envelope = Envelope::<EnvelopeType0>::new(&response_sym_key, response)?;
        let base64_notification =
            base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

        info!("Publishing response on topic {response_topic}");
        publish_relay_message(
            &state.relay_http_client,
            &Publish {
                topic: response_topic.into(),
                message: base64_notification.into(),
                tag: NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_TAG,
                ttl_secs: NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_TTL.as_secs() as u32,
                prompt: false,
            },
            state.metrics.as_ref(),
        )
        .await?;
    }

    Ok(())
}

#[instrument(skip(postgres, metrics))]
pub async fn collect_subscriptions(
    account: AccountId,
    app_domain: Option<&str>,
    postgres: &PgPool,
    metrics: Option<&Metrics>,
) -> Result<Vec<NotifyServerSubscription>> {
    info!("Called collect_subscriptions");

    let subscriptions = if let Some(app_domain) = app_domain {
        get_subscriptions_by_account_and_app(account, app_domain, postgres, metrics).await?
    } else {
        get_subscriptions_by_account(account, postgres, metrics).await?
    };

    let subscriptions = {
        let try_subscriptions = subscriptions
            .into_iter()
            .map(|sub| {
                fn wrap(sub: SubscriberWithProject) -> Result<NotifyServerSubscription> {
                    Ok(NotifyServerSubscription {
                        app_domain: sub.app_domain,
                        app_authentication_key: format!(
                            "did:key:{}",
                            DecodedClientId(decode_key(&sub.authentication_public_key)?)
                        ),
                        sym_key: sub.sym_key,
                        account: sub.account,
                        scope: sub.scope,
                        expiry: sub.expiry.timestamp() as u64,
                    })
                }
                wrap(sub)
            })
            .collect::<Vec<_>>();
        let mut subscriptions = Vec::with_capacity(try_subscriptions.len());
        for result in try_subscriptions {
            subscriptions.push(result?);
        }
        subscriptions
    };

    Ok(subscriptions)
}

// TODO do async outside of websocket request handler
#[instrument(skip_all, fields(account = %account, app_domain = %app_domain))]
pub async fn update_subscription_watchers(
    account: AccountId,
    app_domain: &str,
    postgres: &PgPool,
    http_client: &relay_client::http::Client,
    metrics: Option<&Metrics>,
    authentication_secret: &ed25519_dalek::SigningKey,
    authentication_public: &ed25519_dalek::VerifyingKey,
) -> Result<()> {
    info!("Called update_subscription_watchers");

    let identity: DecodedClientId = DecodedClientId(authentication_public.to_bytes());
    let notify_did_key = format!("did:key:{identity}");

    let did_pkh = format!("did:pkh:{account}");

    #[allow(clippy::too_many_arguments)]
    #[instrument(skip_all, fields(did_pkh = %did_pkh, aud = %aud, subscriptions_count = %subscriptions.len()))]
    async fn send(
        subscriptions: Vec<NotifyServerSubscription>,
        aud: String,
        sym_key: &str,
        notify_did_key: String,
        did_pkh: String,
        http_client: &relay_client::http::Client,
        metrics: Option<&Metrics>,
        authentication_secret: &ed25519_dalek::SigningKey,
    ) -> Result<()> {
        let now = Utc::now();
        let response_message = WatchSubscriptionsChangedRequestAuth {
            shared_claims: SharedClaims {
                iat: now.timestamp() as u64,
                exp: add_ttl(now, NOTIFY_SUBSCRIPTIONS_CHANGED_TTL).timestamp() as u64,
                iss: notify_did_key.clone(),
                act: "notify_subscriptions_changed".to_string(),
                aud,
            },
            sub: did_pkh,
            sbs: subscriptions,
        };
        let auth = sign_jwt(response_message, authentication_secret)?;
        let request = NotifyRequest::new(
            NOTIFY_SUBSCRIPTIONS_CHANGED_METHOD,
            json!({ "subscriptionsChangedAuth": auth }),
        ); // TODO use structure

        let sym_key = decode_key(sym_key)?;
        let envelope = Envelope::<EnvelopeType0>::new(&sym_key, request)?;
        let base64_notification =
            base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

        let topic = Topic::from(sha256::digest(&sym_key));
        publish_relay_message(
            http_client,
            &Publish {
                topic,
                message: base64_notification.into(),
                tag: NOTIFY_SUBSCRIPTIONS_CHANGED_TAG,
                ttl_secs: NOTIFY_SUBSCRIPTIONS_CHANGED_TTL.as_secs() as u32,
                prompt: false,
            },
            metrics,
        )
        .await?;

        Ok(())
    }

    let all_account_subscriptions =
        collect_subscriptions(account.clone(), None, postgres, metrics).await?;

    let app_subscriptions = all_account_subscriptions
        .iter()
        .filter(|sub| sub.app_domain == app_domain)
        .cloned()
        .collect::<Vec<_>>();

    let subscription_watchers = get_subscription_watchers_for_account_by_app_or_all_app(
        account, app_domain, postgres, metrics,
    )
    .await?;
    for watcher in subscription_watchers {
        let subscriptions = if watcher.project.is_some() {
            app_subscriptions.clone()
        } else {
            all_account_subscriptions.clone()
        };

        send(
            subscriptions,
            watcher.did_key.clone(),
            &watcher.sym_key,
            notify_did_key.clone(),
            did_pkh.clone(),
            http_client,
            metrics,
            authentication_secret,
        )
        .await?
    }

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
) -> std::result::Result<(), CheckAppAuthorizationError> {
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
