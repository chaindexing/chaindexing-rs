use crate::RepoError;

#[derive(Debug)]
pub enum IngesterError {
    RepoConnectionError,
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
