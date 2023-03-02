use {
    crate::{error::Result, state::AppState},
    axum::{
        extract::State,
        http::StatusCode,
        response::{IntoResponse, Response},
    },
    std::sync::Arc,
};

pub async fn handler(State(state): State<Arc<AppState>>) -> Result<Response> {
    if let Some(metrics) = &state.metrics {
        let exported = metrics.export()?;

        Ok((StatusCode::OK, exported).into_response())
    } else {
        // No Metrics!
        Ok((StatusCode::BAD_REQUEST, "Metrics not enabled.".to_string()).into_response())
    }
}
