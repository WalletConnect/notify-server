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
};

pub async fn handle(
    msg: relay_client::websocket::PublishedMessage,
    state: &Arc<AppState>,
    client: &Arc<relay_client::websocket::Client>,
) -> Result<()> {
    let request_id = uuid::Uuid::new_v4();
    let topic = msg.topic.clone();
    let _span = tracing::info_span!(
        "wc_notifyWatchSubscriptions", topic = %topic, request_id = %request_id,
    )
    .entered();

    {
        if msg.topic != state.notify_keys.key_agreement_topic {
            return Err(Error::WrongNotifyWatchSubscriptionsTopic(msg.topic));
        }
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
        if request_auth.act != "notify_watch_subscriptions" {
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

    let app_url = match authorization.app {
        AuthorizedApp::Unlimited => None,
        AuthorizedApp::Limited(app_url) => Some(app_url),
    };

    let subscriptions =
        collect_subscriptions(account, app_url.as_deref(), state.database.as_ref()).await?;

    state
        .database
        .collection("watch_subscriptions")
        .insert_one(
            WatchSubscriptionsEntry {
                account: account.clone(),
                app_url,
                sym_key: hex::encode(response_sym_key),
                did_key: request_auth.shared_claims.iss.clone(),
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
            },
            act: "notify_watch_subscriptions_response".to_string(),
            aud: request_auth.shared_claims.iss,
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
    let mut lookup_data = database
        .collection::<LookupEntry>("lookup_table")
        .find(doc! { "account": account }, None)
        .await?;

    let mut subscriptions = vec![];

    while let Some(lookup_entry) = lookup_data.try_next().await? {
        let project_id = lookup_entry.project_id;

        let project_data = database
            .collection::<ProjectData>("project_data")
            .find_one(doc!("_id": &project_id), None)
            .await?
            .ok_or_else(|| crate::error::Error::NoProjectDataForProjectId(project_id.clone()))?;

        if let Some(domain) = app_domain {
            // TODO make domain a dedicated field and query by it when app_domain is Some()
            if project_data.app_domain()? != domain {
                continue;
            }
        }

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
            dapp_url: project_data.dapp_url,
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
    app_url: &str,
    database: &Database,
    client: &relay_client::websocket::Client,
    authentication_secret: &ed25519_dalek::SigningKey,
    authentication_public: &ed25519_dalek::VerifyingKey,
) -> Result<()> {
    let identity: DecodedClientId = DecodedClientId(authentication_public.to_bytes());
    let did_key = format!("did:key:{identity}");

    let account_id = format!("did:pkh:{account}");

    async fn send(
        subscriptions: Vec<NotifyServerSubscription>,
        aud: String,
        sym_key: &str,
        did_key: String,
        account_id: String,
        client: &relay_client::websocket::Client,
        authentication_secret: &ed25519_dalek::SigningKey,
    ) -> Result<()> {
        let now = Utc::now();
        let response_message = WatchSubscriptionsChangedRequestAuth {
            shared_claims: SharedClaims {
                iat: now.timestamp() as u64,
                exp: add_ttl(now, NOTIFY_SUBSCRIPTIONS_CHANGED_TTL).timestamp() as u64,
                iss: did_key.clone(),
            },
            act: "notify_subscriptions_changed_request".to_string(),
            aud,
            sub: account_id,
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

        client
            .publish(
                Topic::from(sha256::digest(&sym_key)),
                base64_notification,
                NOTIFY_SUBSCRIPTIONS_CHANGED_TAG,
                NOTIFY_SUBSCRIPTIONS_CHANGED_TTL,
                false,
            )
            .await?;

        Ok(())
    }

    let mut cursor = database
        .collection::<WatchSubscriptionsEntry>("watch_subscriptions")
        .find(doc! { "account": account, "app_url": Some(app_url) }, None)
        .await?;

    let subscriptions = collect_subscriptions(account, Some(app_url), database).await?;
    while let Some(watch_subscriptions_entry) = cursor.try_next().await? {
        send(
            subscriptions.clone(),
            watch_subscriptions_entry.did_key.clone(),
            &watch_subscriptions_entry.sym_key,
            did_key.clone(),
            account_id.clone(),
            client,
            authentication_secret,
        )
        .await?;
    }

    let mut cursor = database
        .collection::<WatchSubscriptionsEntry>("watch_subscriptions")
        .find(doc! { "account": account, "app_url": None::<&str> }, None)
        .await?;

    let subscriptions = collect_subscriptions(account, None, database).await?;
    while let Some(watch_subscriptions_entry) = cursor.try_next().await? {
        send(
            subscriptions.clone(),
            watch_subscriptions_entry.did_key.clone(),
            &watch_subscriptions_entry.sym_key,
            did_key.clone(),
            account_id.clone(),
            client,
            authentication_secret,
        )
        .await?;
    }

    Ok(())
}
