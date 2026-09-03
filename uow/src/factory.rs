use crate::{platform::DatabasePlatform, tx_result::TxResult, uow::UnitOfWork};

/// Factory that creates unit of work instances and manages their lifecycle.
/// The closure passed to `with_unitofwork` returns `Result<TxResult<T>, TxResult<E>>`,
/// allowing the caller to indicate whether the transaction should be committed or rolled back.
pub trait UnitOfWorkFactory<P: DatabasePlatform>: Send + Sync {
    /// The unit of work type for this platform.
    type UoW: UnitOfWork<P>;

    /// Executes the given async closure within a unit of work.
    ///
    /// The closure receives `&mut Self::UoW` and returns `Result<TxResult<T>, TxResult<E>>`.
    /// If the closure returns `Commit`, the transaction is committed.
    /// If the closure returns `Rollback`, the transaction is rolled back.
    async fn with_unitofwork<T, E, F>(&self, f: F) -> Result<T, E>
    where
        T: Send,
        E: Send,
        F: AsyncFnOnce(&mut Self::UoW) -> Result<TxResult<T>, TxResult<E>> + Send;
}
