use {
    axum::response::IntoResponse,
    hyper::StatusCode,
    tracing::error,
    wc::metrics::{otel::metrics::Counter, ServiceMetrics},
};

#[derive(Clone)]
pub struct Metrics {
    pub processed_notifications: Counter<u64>,
}

impl Metrics {
    pub fn new() -> Self {
        let meter = wc::metrics::ServiceMetrics::meter();
        let processed_notifications = meter
            .u64_counter("processed_notifications")
            .with_description("The number of processed notifications")
            .init();
        Metrics {
            processed_notifications,
        }
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn handler() -> impl IntoResponse {
    match ServiceMetrics::export() {
        Ok(content) => (StatusCode::OK, content),
        Err(e) => {
            error!(?e, "Failed to parse metrics");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to parse metrics".to_string(),
            )
        }
    }
}
