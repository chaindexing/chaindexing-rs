use crate::RepoError;

use super::provider::ProviderError;

#[derive(Debug)]
pub enum IngesterError {
    RepoError(RepoError),
    ProviderError(ProviderError),
    GenericError(String),
}

impl From<RepoError> for IngesterError {
    fn from(value: RepoError) -> Self {
        IngesterError::RepoError(value)
    }
}

impl From<ProviderError> for IngesterError {
    fn from(value: ProviderError) -> Self {
        IngesterError::ProviderError(value)
    }
}

impl std::fmt::Display for IngesterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IngesterError::RepoError(error) => write!(f, "{error}"),
            IngesterError::ProviderError(error) => write!(f, "provider error: {error}"),
            IngesterError::GenericError(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for IngesterError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            IngesterError::RepoError(error) => Some(error),
            IngesterError::ProviderError(error) => Some(error),
            IngesterError::GenericError(_) => None,
        }
    }
}
