#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Envy(#[from] envy::Error),

    #[error(transparent)]
    Trace(#[from] opentelemetry::trace::TraceError),

    #[error(transparent)]
    Metrics(#[from] opentelemetry::metrics::MetricsError),

    #[error(transparent)]
    Database(#[from] sqlx::Error),

    #[error("database migration failed: {0}")]
    DatabaseMigration(#[from] sqlx::migrate::MigrateError),

    #[error(transparent)]
    Store(#[from] crate::stores::StoreError),
}
