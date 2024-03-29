use {
    crate::{
        error::NotifyServerError,
        model::helpers::upsert_project,
        publish_relay_message::subscribe_relay_topic,
        rate_limit::{self, Clock, RateLimitError},
        registry::{extractor::AuthedProjectId, storage::redis::Redis},
        state::AppState,
        utils::topic_from_key,
    },
    axum::{
        self,
        extract::State,
        response::{IntoResponse, Response},
        Json,
    },
    chacha20poly1305::aead::OsRng,
    hyper::StatusCode,
    once_cell::sync::Lazy,
    regex::Regex,
    relay_rpc::{auth::ed25519_dalek::SigningKey, domain::ProjectId},
    serde::{Deserialize, Serialize},
    serde_json::json,
    std::sync::Arc,
    tracing::{info, instrument},
    x25519_dalek::{PublicKey, StaticSecret},
};

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SubscribeTopicRequestBody {
    pub app_domain: Arc<str>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SubscribeTopicResponseBody {
    pub authentication_key: String,
    pub subscribe_key: String,
}

#[instrument(name = "notify_v1", skip(state, subscribe_topic_data))]
pub async fn handler(
    State(state): State<Arc<AppState>>,
    AuthedProjectId(project_id, _): AuthedProjectId,
    Json(subscribe_topic_data): Json<SubscribeTopicRequestBody>,
) -> Result<Response, NotifyServerError> {
    // let _span = tracing::info_span!(
    //     "subscribe_topic", project_id = %project_id,
    // )
    // .entered();

    if let Some(redis) = state.redis.as_ref() {
        subscribe_topic_rate_limit(redis, &project_id, &state.clock).await?;
    }

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
    let topic = topic_from_key(signing_public.as_bytes());

    let authentication_key = SigningKey::generate(&mut OsRng);

    let project = upsert_project(
        project_id,
        &app_domain,
        topic.clone(),
        &authentication_key,
        &subscribe_key,
        &state.postgres,
        state.metrics.as_ref(),
    )
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(e)
            if e.is_unique_violation() && e.message().contains("project_app_domain_key") =>
        {
            NotifyServerError::AppDomainInUseByAnotherProject
        }
        other => other.into(),
    })?;

    let topic = project.topic.into();
    info!("Subscribing to project topic: {topic}");
    subscribe_relay_topic(&state.relay_client, &topic, state.metrics.as_ref()).await?;

    info!("Successfully subscribed to project topic: {topic}");
    Ok(Json(SubscribeTopicResponseBody {
        authentication_key: project.authentication_public_key,
        subscribe_key: project.subscribe_public_key,
    })
    .into_response())
}

pub async fn subscribe_topic_rate_limit(
    redis: &Arc<Redis>,
    project_id: &ProjectId,
    clock: &Clock,
) -> Result<(), RateLimitError> {
    rate_limit::token_bucket(
        redis,
        format!("subscribe_topic-{project_id}"),
        100,
        chrono::Duration::minutes(1),
        1,
        clock,
    )
    .await
}

fn is_domain(domain: &str) -> bool {
    static DOMAIN_REGEX: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"^[a-z0-9-_\.]+$").expect("Safe unwrap: panics should be caught by test cases")
    });
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
