use thiserror::Error;
use uow::UoW;

#[derive(Error, Debug)]
pub enum StorageError<U: UoW> {
    #[error("storage error")]
    PlatformError(#[source] U::Error),

    #[error("failed to interpret stored data")]
    StorageFormatError(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),
}
