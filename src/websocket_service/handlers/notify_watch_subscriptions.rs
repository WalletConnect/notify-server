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
        handlers::subscribe_topic::ProjectData,
        spec::{
            NOTIFY_SUBSCRIPTIONS_CHANGED_METHOD,
            NOTIFY_SUBSCRIPTIONS_CHANGED_TAG,
            NOTIFY_SUBSCRIPTIONS_CHANGED_TTL,
            NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_TAG,
            NOTIFY_WATCH_SUBSCRIPTIONS_RESPONSE_TTL,
        },
        state::AppState,
        types::{
            ClientData,
            Envelope,
            EnvelopeType0,
            EnvelopeType1,
            LookupEntry,
            WatchSubscriptionsEntry,
        },
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
    futures::TryStreamExt,
    mongodb::{bson::doc, Database},
    relay_rpc::domain::{DecodedClientId, Topic},
    serde_json::{json, Value},
    std::sync::Arc,
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
    let account = &authorization.account;

    let app_domain = match authorization.app {
        AuthorizedApp::Unlimited => None,
        AuthorizedApp::Limited(app_domain) => Some(app_domain),
    };
    info!("app_domain: {app_domain:?}");

    let subscriptions =
        collect_subscriptions(account, app_domain.as_deref(), state.database.as_ref()).await?;

    let did_key = request_auth.shared_claims.iss;

    // TODO txn

    let delete_result = state
        .database
        .collection::<WatchSubscriptionsEntry>("watch_subscriptions")
        .delete_many(doc! { "did_key": &did_key }, None)
        .await?;
    info!("deleted_count: {}", delete_result.deleted_count);

    state
        .database
        .collection("watch_subscriptions")
        .insert_one(
            WatchSubscriptionsEntry {
                account: account.clone(),
                app_domain,
                sym_key: hex::encode(response_sym_key),
                did_key: did_key.clone(),
                expiry: (Utc::now() + Duration::minutes(5)).timestamp() as u64,
            },
            None,
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
    account: &str,
    app_domain: Option<&str>,
    database: &Database,
) -> Result<Vec<NotifyServerSubscription>> {
    let _span = tracing::info_span!(
        "collect_subscriptions", account = %account, app_domain = ?app_domain,
    )
    .entered();

    info!("Called collect_subscriptions");

    let mut lookup_data = database
        .collection::<LookupEntry>("lookup_table")
        .find(doc! { "account": account }, None)
        .await?;

    let mut subscriptions = vec![];

    while let Some(lookup_entry) = lookup_data.try_next().await? {
        let project_id = lookup_entry.project_id;
        info!("project_id: {project_id}");

        let project_data = database
            .collection::<ProjectData>("project_data")
            .find_one(doc!("_id": &project_id), None)
            .await?
            .ok_or_else(|| crate::error::Error::NoProjectDataForProjectId(project_id.clone()))?;

        if let Some(domain) = app_domain {
            info!("app_domain is Some");
            // TODO make domain a dedicated field and query by it when app_domain is Some()
            if project_data.app_domain != domain {
                info!("project app_domain does not match");
                continue;
            }
        }

        info!("querying project_id database for client_data");
        let client_data = database
            .collection::<ClientData>(&project_id)
            .find_one(doc!("_id": account), None)
            .await?
            .ok_or_else(|| {
                crate::error::Error::NoClientDataForProjectIdAndAccount(
                    project_id.clone(),
                    account.to_owned(),
                )
            })?;

        subscriptions.push(NotifyServerSubscription {
            app_domain: project_data.app_domain,
            account: account.to_owned(),
            scope: client_data.scope,
            sym_key: client_data.sym_key,
            expiry: client_data.expiry,
        });
    }

    Ok(subscriptions)
}

pub async fn update_subscription_watchers(
    account: &str,
    app_domain: &str,
    database: &Database,
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

    info!("querying watch_subscriptions for app_domain: {app_domain}");
    let mut cursor = database
        .collection::<WatchSubscriptionsEntry>("watch_subscriptions")
        .find(
            doc! { "account": account, "app_domain": Some(app_domain) },
            None,
        )
        .await?;

    let subscriptions = collect_subscriptions(account, Some(app_domain), database).await?;
    while let Some(watch_subscriptions_entry) = cursor.try_next().await? {
        send(
            subscriptions.clone(),
            watch_subscriptions_entry.did_key.clone(),
            &watch_subscriptions_entry.sym_key,
            notify_did_key.clone(),
            did_pkh.clone(),
            client,
            authentication_secret,
        )
        .await?;
    }

    info!("querying watch_subscriptions for no app_domain");
    let mut cursor = database
        .collection::<WatchSubscriptionsEntry>("watch_subscriptions")
        .find(
            doc! { "account": account, "app_domain": None::<&str> },
            None,
        )
        .await?;

    let subscriptions = collect_subscriptions(account, None, database).await?;
    while let Some(watch_subscriptions_entry) = cursor.try_next().await? {
        send(
            subscriptions.clone(),
            watch_subscriptions_entry.did_key.clone(),
            &watch_subscriptions_entry.sym_key,
            notify_did_key.clone(),
            did_pkh.clone(),
            client,
            authentication_secret,
        )
        .await?;
    }

    Ok(())
}
