use {
    crate::state::AppState,
    axum::{extract::State, http::StatusCode, response::IntoResponse},
    std::sync::Arc,
};

pub async fn handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    (
        StatusCode::OK,
        format!(
            "OK, {} v{}",
            state.build_info.crate_info.name, state.build_info.crate_info.version
        ),
    )
}
