use {
    crate::error::NotifyServerError,
    relay_client::{http::Client, ConnectionOptions},
    relay_rpc::{
        auth::{ed25519_dalek::SigningKey, AuthToken},
        domain::ProjectId,
        user_agent::ValidUserAgent,
    },
    std::time::Duration,
    url::Url,
};

pub fn create_http_client(
    keypair: &SigningKey,
    relay_url: Url,
    notify_url: Url,
    project_id: ProjectId,
) -> Result<Client, NotifyServerError> {
    Ok(Client::new(&create_http_connect_options(
        keypair, relay_url, notify_url, project_id,
    )?)?)
}

pub fn create_http_connect_options(
    keypair: &SigningKey,
    mut relay_url: Url,
    notify_url: Url,
    project_id: ProjectId,
) -> Result<ConnectionOptions, NotifyServerError> {
    // TODO remove once switched to https
    relay_url
        .set_scheme(&relay_url.scheme().replace("ws", "http"))
        .map_err(|_| NotifyServerError::UrlSetScheme)?;

    let rpc_address = relay_url.join("/rpc")?;
    Ok(
        // HTTP client cannot currently use an expiring JWT because the same relay client is used for the entire duration of the process
        create_connect_options(keypair, &relay_url, notify_url, project_id, None)?
            .with_address(rpc_address),
    )
}

fn create_connect_options(
    keypair: &SigningKey,
    relay_url: &Url,
    notify_url: Url,
    project_id: ProjectId,
    ttl: Option<Duration>,
) -> Result<ConnectionOptions, NotifyServerError> {
    let auth = AuthToken::new(notify_url.clone())
        .aud(relay_url.origin().ascii_serialization())
        .ttl(ttl)
        .as_jwt(keypair)?;

    let user_agent = relay_rpc::user_agent::UserAgent::ValidUserAgent(ValidUserAgent {
        protocol: relay_rpc::user_agent::Protocol {
            kind: relay_rpc::user_agent::ProtocolKind::WalletConnect,
            version: 2,
        },
        sdk: relay_rpc::user_agent::Sdk {
            language: relay_rpc::user_agent::SdkLanguage::Rust,
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
        os: relay_rpc::user_agent::OsInfo {
            os_family: "ECS".into(),
            ua_family: None,
            version: None,
        },
        id: Some(relay_rpc::user_agent::Id {
            environment: relay_rpc::user_agent::Environment::Unknown("Notify Server".into()),
            host: Some(notify_url.to_string()),
        }),
    });

    Ok(ConnectionOptions::new(project_id, auth).with_user_agent(user_agent))
}
