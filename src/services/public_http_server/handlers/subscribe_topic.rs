use {
    crate::{
        error::Result, model::helpers::upsert_project, registry::extractor::AuthedProjectId,
        state::AppState,
    },
    axum::{self, extract::State, response::IntoResponse, Json},
    chacha20poly1305::aead::OsRng,
    hyper::StatusCode,
    once_cell::sync::Lazy,
    regex::Regex,
    relay_rpc::domain::Topic,
    serde::{Deserialize, Serialize},
    serde_json::json,
    std::sync::Arc,
    tracing::{info, instrument},
    x25519_dalek::{PublicKey, StaticSecret},
};

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SubscribeTopicRequestData {
    pub app_domain: String,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SubscribeTopicResponseData {
    authentication_key: String,
    subscribe_key: String,
}

// TODO test idempotency

#[instrument(name = "notify_v1", skip(state, subscribe_topic_data))]
pub async fn handler(
    State(state): State<Arc<AppState>>,
    AuthedProjectId(project_id, _): AuthedProjectId,
    Json(subscribe_topic_data): Json<SubscribeTopicRequestData>,
) -> Result<axum::response::Response> {
    // let _span = tracing::info_span!(
    //     "subscribe_topic", project_id = %project_id,
    // )
    // .entered();

    let app_domain = subscribe_topic_data.app_domain;
    if app_domain.len() > 253 {
        // Domains max at 253 chars according to: https://en.wikipedia.org/wiki/Hostname
        // Conveniently, that fits into a varchar(255) column
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"app_domain exceeds 253 characters"})),
        )
            .into_response());
    }
    if !is_domain(&app_domain) {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"app_domain is not a valid domain"})),
        )
            .into_response());
    }

    info!("Getting or generating keypair for project: {project_id} and domain: {app_domain}");

    let subscribe_key = StaticSecret::random_from_rng(OsRng);
    let signing_public = PublicKey::from(&subscribe_key);
    let topic: Topic = sha256::digest(signing_public.as_bytes()).into();

    let authentication_key = ed25519_dalek::SigningKey::generate(&mut OsRng);

    let project = upsert_project(
        project_id,
        &app_domain,
        topic.clone(),
        &authentication_key,
        &subscribe_key,
        &state.postgres,
    )
    .await?;
    // TODO handle duplicate app_domain error

    info!("Subscribing to project topic: {topic}");
    state.relay_ws_client.subscribe(topic).await?;

    Ok(Json(SubscribeTopicResponseData {
        authentication_key: project.authentication_public_key,
        subscribe_key: project.subscribe_public_key,
    })
    .into_response())
}

fn is_domain(domain: &str) -> bool {
    static DOMAIN_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-z0-9-_\.]+$").unwrap());
    DOMAIN_REGEX.is_match(domain)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn valid_domains() {
        assert!(is_domain("com"));
        assert!(is_domain("example.com"));
        assert!(is_domain("app.example.com"));
        assert!(is_domain("123.example.com"));
    }

    #[test]
    fn not_valid_domains() {
        assert!(!is_domain("https://app.example.com"));
        assert!(!is_domain("app.example.com/"));
        assert!(!is_domain(" app.example.com"));
        assert!(!is_domain("app.example.com "));
    }
}
