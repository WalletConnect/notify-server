use {
    crate::{
        auth::{
            add_ttl,
            from_jwt,
            sign_jwt,
            verify_identity,
            AuthError,
            AuthorizedApp,
            NotifyServerSubscription,
            SharedClaims,
            WatchSubscriptionsChangedRequestAuth,
            WatchSubscriptionsRequestAuth,
            WatchSubscriptionsResponseAuth,
        },
        error::Error,
        model::{
            helpers::{
                get_project_by_app_domain,
                get_subscription_watchers_for_account_by_app_or_all_app,
                get_subscriptions_by_account,
                get_subscriptions_by_account_and_app,
                upsert_subscription_watcher,
            },
            types::AccountId,
        },
        spec::{
            NOTIFY_SUBSCRIPTIONS_CHANGED_METHOD,
            NOTIFY_SUBSCRIPTIONS_CHANGED_TAG,
            NOTIFY_SUBSCRIPTIONS_CHANGED_TTL,
            NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_TAG,
            NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_TTL,
        },
        state::AppState,
        types::{Envelope, EnvelopeType0, EnvelopeType1},
        websocket_service::{
            decode_key,
            derive_key,
            handlers::decrypt_message,
            NotifyRequest,
            NotifyResponse,
            NotifyWatchSubscriptions,
        },
        Result,
    },
    base64::Engine,
    chrono::{Duration, Utc},
    relay_rpc::domain::{DecodedClientId, Topic},
    serde_json::{json, Value},
    sqlx::PgPool,
    std::{collections::HashSet, sync::Arc},
    tracing::info,
};

pub async fn handle(
    msg: relay_client::websocket::PublishedMessage,
    state: &Arc<AppState>,
    client: &Arc<relay_client::websocket::Client>,
) -> Result<()> {
    let _span = tracing::info_span!("wc_notifyWatchSubscriptions").entered();

    if msg.topic != state.notify_keys.key_agreement_topic {
        return Err(Error::WrongNotifyWatchSubscriptionsTopic(msg.topic));
    }

    let envelope = Envelope::<EnvelopeType1>::from_bytes(
        base64::engine::general_purpose::STANDARD.decode(msg.message.to_string())?,
    )?;

    let client_public_key = x25519_dalek::PublicKey::from(envelope.pubkey());
    let response_sym_key = derive_key(&client_public_key, &state.notify_keys.key_agreement_secret)?;
    let response_topic = sha256::digest(&response_sym_key);

    let msg: NotifyRequest<NotifyWatchSubscriptions> =
        decrypt_message(envelope, &response_sym_key)?;

    let id = msg.id;

    let request_auth =
        from_jwt::<WatchSubscriptionsRequestAuth>(&msg.params.watch_subscriptions_auth)?;

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

        // TODO merge code with integration.rs#verify_jwt()
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

    let subscriptions =
        collect_subscriptions(account.clone(), app_domain.as_deref(), &state.postgres).await?;

    let did_key = request_auth.shared_claims.iss;
    info!("did_key: {did_key}");

    let project = if let Some(app_domain) = app_domain {
        let project = get_project_by_app_domain(&app_domain, &state.postgres)
            .await
            .map_err(|e| match e {
                sqlx::Error::RowNotFound => Error::NoProjectDataForAppDomain(app_domain),
                e => e.into(),
            })?;
        Some(project.id)
    } else {
        None
    };
    upsert_subscription_watcher(
        account,
        project,
        &did_key,
        &hex::encode(response_sym_key),
        Utc::now() + Duration::days(1),
        &state.postgres,
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
                aud: did_key,
            },
            sub: request_auth.sub,
            sbs: subscriptions,
        };
        let response_auth = sign_jwt(response_message, &state.notify_keys.authentication_secret)?;
        let response = NotifyResponse::<Value> {
            id,
            jsonrpc: "2.0".into(),
            result: json!({ "responseAuth": response_auth }), // TODO use structure
        };

        let envelope = Envelope::<EnvelopeType0>::new(&response_sym_key, response)?;
        let base64_notification =
            base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

        info!("Publishing response on topic {response_topic}");
        client
            .publish(
                response_topic.into(),
                base64_notification,
                NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_TAG,
                NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_TTL,
                false,
            )
            .await?;
    }

    Ok(())
}

pub async fn collect_subscriptions(
    account: AccountId,
    app_domain: Option<&str>,
    postgres: &PgPool,
) -> Result<Vec<NotifyServerSubscription>> {
    let _span = tracing::info_span!(
        "collect_subscriptions", account = %account, app_domain = ?app_domain,
    )
    .entered();

    info!("Called collect_subscriptions");

    let subscriptions = if let Some(app_domain) = app_domain {
        get_subscriptions_by_account_and_app(account, app_domain, postgres).await?
    } else {
        get_subscriptions_by_account(account, postgres).await?
    };

    let subscriptions = subscriptions
        .into_iter()
        .map(|sub| NotifyServerSubscription {
            app_domain: sub.app_domain,
            sym_key: sub.sym_key,
            account: sub.account,
            scope: HashSet::new(), // FIXME scope
            expiry: sub.expiry.timestamp() as u64,
        })
        .collect::<Vec<_>>();

    Ok(subscriptions)
}

// TODO do async outside of websocket request handler
pub async fn update_subscription_watchers(
    account: AccountId,
    app_domain: &str,
    postgres: &PgPool,
    client: &relay_client::websocket::Client,
    authentication_secret: &ed25519_dalek::SigningKey,
    authentication_public: &ed25519_dalek::VerifyingKey,
) -> Result<()> {
    let _span = tracing::info_span!(
        "update_subscription_watchers", account = %account, app_domain = ?app_domain,
    )
    .entered();

    info!("Called update_subscription_watchers");

    let identity: DecodedClientId = DecodedClientId(authentication_public.to_bytes());
    let notify_did_key = format!("did:key:{identity}");

    let did_pkh = format!("did:pkh:{account}");

    async fn send(
        subscriptions: Vec<NotifyServerSubscription>,
        aud: String,
        sym_key: &str,
        notify_did_key: String,
        did_pkh: String,
        client: &relay_client::websocket::Client,
        authentication_secret: &ed25519_dalek::SigningKey,
    ) -> Result<()> {
        let _span = tracing::info_span!("sending wc_notifySubscriptionsChanged", did_pkh = %did_pkh, aud = %aud, subscriptions_count = subscriptions.len()).entered();

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
        info!("topic: {topic}");
        client
            .publish(
                topic,
                base64_notification,
                NOTIFY_SUBSCRIPTIONS_CHANGED_TAG,
                NOTIFY_SUBSCRIPTIONS_CHANGED_TTL,
                false,
            )
            .await?;

        Ok(())
    }

    let all_account_subscriptions = collect_subscriptions(account.clone(), None, postgres).await?;

    let app_subscriptions = all_account_subscriptions
        .iter()
        .filter(|sub| sub.app_domain == app_domain)
        .cloned()
        .collect::<Vec<_>>();

    let subscription_watchers =
        get_subscription_watchers_for_account_by_app_or_all_app(account, app_domain, postgres)
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
            client,
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
