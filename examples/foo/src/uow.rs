use async_trait::async_trait;

/// Trait for a Unit of Work (transaction wrapper).
/// Allows different transaction backends to be plugged in.
#[async_trait]
pub trait UoW: Send {
    /// The error type returned by this UoW implementation.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Commit the transaction.
    async fn commit(self) -> Result<(), Self::Error>;
}

/// Trait for a Unit of Work factory.
/// Creates UoW instances with a specific error type.
#[async_trait]
pub trait UoWFac: Clone + Send + Sync {
    /// The type of UoW this factory creates.
    type UoW: UoW;

    /// Create a new UoW instance.
    async fn create(&self) -> Result<Self::UoW, <Self::UoW as UoW>::Error>;

    /// Execute a closure within a UoW, automatically committing on success
    /// or rolling back on failure.
    ///
    /// The closure takes ownership of the UoW and returns it along with the result,
    /// avoiding lifetime issues with async closures.
    async fn with_uow<F, Fut, R, E>(&self, f: F) -> Result<Result<R, E>, <Self::UoW as UoW>::Error>
    where
        F: FnOnce(Self::UoW) -> Fut + Send,
        Fut: std::future::Future<Output = (Self::UoW, Result<TxResult<R>, TxResult<E>>)> + Send,
        R: Send,
        E: Send;
}

pub enum TxResult<T> {
    Commit(T),
    Rollback(T)
}
