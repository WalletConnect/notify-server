//! Error typedefs used by this crate

use thiserror::Error as ThisError;

/// The error produced from most Storage functions
#[derive(Debug, ThisError)]
pub enum StorageError {
    /// Unable to serialize data to store
    #[error("error on serialize data")]
    Serialize,
    /// Unable to deserialize data from store
    #[error("error on deserialize data")]
    Deserialize,
    /// Error on establishing a connection with the storage
    #[error("error on open connection")]
    Connection(String),
    /// An unexpected error occurred
    #[error("{0:?}")]
    Other(String),
}
