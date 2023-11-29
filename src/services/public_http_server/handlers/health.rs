use {
    crate::state::AppState,
    axum::{extract::State, http::StatusCode, response::IntoResponse},
    std::sync::Arc,
};

// No rate limit necessary since returning a fixed string is less computational intensive than tracking the rate limit

// TODO generate this response at app startup to avoid unnecessary string allocations
pub async fn handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    (
        StatusCode::OK,
        format!(
            "OK, {} v{}",
            state.build_info.crate_info.name, state.build_info.crate_info.version
        ),
    )
}
