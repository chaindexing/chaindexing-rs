use crate::RepoError;

#[derive(Debug)]
pub enum EventsIngesterError {
    RepoConnectionError,
    GenericError(String),
}

impl From<RepoError> for EventsIngesterError {
    fn from(value: RepoError) -> Self {
        match value {
            RepoError::NotConnected => EventsIngesterError::RepoConnectionError,
            RepoError::Unknown(error) => EventsIngesterError::GenericError(error),
        }
    }
}
