use {
    crate::state::AppState,
    async_trait::async_trait,
    axum::{
        extract::{FromRequestParts, Path},
        headers::{authorization::Bearer, Authorization},
        http::request::Parts,
        TypedHeader,
    },
    hyper::StatusCode,
    serde_json::json,
    std::{collections::HashMap, sync::Arc},
    tracing::warn,
};

/// Extracts project_id from uri and project_secret from Authorization header.
/// Verifies their correctness against registry and returns AuthedProjectId
/// struct.
pub struct AuthedProjectId(pub String, pub String);

#[async_trait]
impl FromRequestParts<Arc<AppState>> for AuthedProjectId {
    type Rejection = (StatusCode, String);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let Path(path_args) = Path::<HashMap<String, String>>::from_request_parts(parts, state)
            .await
            .map_err(|_| {
                (
                StatusCode::BAD_REQUEST,
                json!({
                    "reason": "Invalid project_id. Please make sure to include project_id in uri. "
                }).to_string(),
            )
            })?;

        let TypedHeader(project_secret) = TypedHeader::<Authorization::<Bearer>>::from_request_parts(parts, state).await.map_err(|_| {
            (
                StatusCode::UNAUTHORIZED,
                json!({
                    "reason": "Unauthorized. Please make sure to include project secret in Authorization header. "
                }).to_string(),
            )
        })?;

        let project_id = path_args
            .get("project_id")
            .ok_or((
                StatusCode::BAD_REQUEST,
                json!({"reason": "Invalid data for authentication".to_string()}).to_string(),
            ))?
            .to_string();

        let authenticated = state
            .registry
            .is_authenticated(&project_id, project_secret.token())
            .await
            .map_err(|e| {
                warn!(?e, "Failed to authenticate project");
                (
                    StatusCode::BAD_REQUEST,
                    "Invalid data for authentication".to_string(),
                )
            })?;

        if !authenticated {
            return Err((
                StatusCode::UNAUTHORIZED,
                json!({
                    "reason": "Invalid project_secret. Please make sure to include proper project secret in Authorization header."
                })
                .to_string(),
            ));
        };

        Ok(AuthedProjectId(
            project_id,
            project_secret.token().to_string(),
        ))
    }
}
