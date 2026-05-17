use crate::RepoError;

use super::provider::ProviderError;

#[derive(Debug)]
pub enum IngesterError {
    RepoConnectionError,
    ProviderError(String),
    GenericError(String),
}

impl From<RepoError> for IngesterError {
    fn from(value: RepoError) -> Self {
        match value {
            RepoError::NotConnected => IngesterError::RepoConnectionError,
            RepoError::Unknown(error) => IngesterError::GenericError(error),
        }
    }
}

impl From<ProviderError> for IngesterError {
    fn from(value: ProviderError) -> Self {
        IngesterError::ProviderError(value.to_string())
    }
}

impl std::fmt::Display for IngesterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IngesterError::RepoConnectionError => write!(f, "repository connection error"),
            IngesterError::ProviderError(error) => write!(f, "provider error: {error}"),
            IngesterError::GenericError(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for IngesterError {}
