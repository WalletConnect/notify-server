use {
    crate::{error::NotifyServerError, state::AppState},
    axum::{
        extract::State,
        http::StatusCode,
        response::{IntoResponse, Response},
        Json,
    },
    serde_json::json,
    std::sync::Arc,
};

pub async fn handler(State(_state): State<Arc<AppState>>) -> Result<Response, NotifyServerError> {
    // TODO
    let json = json!({});
    Ok((StatusCode::OK, Json(json)).into_response())
}
