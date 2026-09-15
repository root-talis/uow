use async_trait::async_trait;
use thiserror::Error;

/// Marker trait for Unit of Work implementations.
/// Defines the error type used by a specific transaction backend.
///
/// Implementations provide a handle to an active transaction.
/// The `TxManager` controls the lifecycle (commit on success, rollback on failure).
#[async_trait]
pub trait UoW: Send {
    /// The error type returned by this UoW implementation.
    type Error: std::error::Error + Send + Sync + 'static;
}

/// Trait for a Transaction manager.
/// Allows different data source backends to be plugged in.
#[async_trait]
pub trait TxManager: Clone + Send + Sync {
    /// The type of Unit of Work this manager creates.
    type UoW: UoW;

    /// Execute a closure within a Unit of Work, automatically finalizing on success
    /// or rolling back on failure.
    ///
    /// The closure takes ownership of the Unit of Work and returns it along with the result,
    /// avoiding lifetime issues with async closures.
    ///
    /// # Errors
    ///
    /// Returns [`TransactionError`] on platform errors during transaction lifecycle:
    /// - [`TransactionError::BeginFailed`] — failed to start the transaction.
    /// - [`TransactionError::CommitFailed`] — failed to commit the transaction.
    /// - [`TransactionError::RollbackFailed`] — failed to roll back the transaction.
    #[must_use]
    async fn transaction<F, Fut, R, E>(
        &self,
        callback: F,
    ) -> Result<Result<R, E>, TransactionError<Self::UoW>>
    where
        F: FnOnce(Self::UoW) -> Fut + Send,
        Fut: std::future::Future<Output = (Self::UoW, Result<R, E>)> + Send,
        R: Send,
        E: Send;

    /// Return an IsolationLevelSelector for the underlying platform.
    /// Attempts to set the provided isolation level.
    ///
    /// The original manager does not change. The selector overrides the isolation level
    /// only for the new manager that it is going to create.
    ///
    /// ```rust,ignore
    /// self.tx_man
    ///     .isolated_at(IsolationLevel::RepeatableRead)
    ///         .or(IsolationLevel::Serializable)
    ///         .if_platform_supports()?
    ///     .transaction(|mut uow| async {
    ///       // ...
    ///     })
    /// ```
    #[must_use]
    fn isolated_at(&self, level: IsolationLevel) -> impl IsolationLevelSelector<Self>;
}

/// Represents failure modes of TxManager::transaction internal logic.
#[derive(Error, Debug)]
pub enum TransactionError<U: UoW> {
    /// A platform error occurred while beginning the transaction.
    #[error("failed to start a unit of work")]
    BeginFailed(#[source] U::Error),

    /// A platform error occurred while committing the transaction.
    #[error("failed to commit a unit of work")]
    CommitFailed(#[source] U::Error),

    /// A platform error occurred while rolling back the transaction.
    #[error("failed to roll back a unit of work")]
    RollbackFailed(#[source] U::Error),
}

impl<U: UoW> TransactionError<U> {
    /// Returns platform error discarding details about its origin.
    #[must_use]
    pub fn into_inner(self) -> U::Error {
        match self {
            TransactionError::BeginFailed(e) => e,
            TransactionError::CommitFailed(e) => e,
            TransactionError::RollbackFailed(e) => e,
        }
    }
}

/// Trait for an Isolation level selector.
///
/// Sets transaction isolation level for a new Transaction manager.
/// Ensures that the underlying platform supports requested isolation level.
/// Supports configuring a chain of fallbacks.
pub trait IsolationLevelSelector<M: TxManager> {
    /// Specifies the next preferable isolation level if previous specified levels
    /// are not supported by the platform.
    ///
    /// Used to configure a chain of fallbacks.
    #[must_use]
    fn or(self, level: IsolationLevel) -> Self;

    /// Returns a new Transaction manager with configured isolation level.
    ///
    /// If none of specified isolation levels are supported by the underlying platform,
    /// returns "IsloationLevelNotSupportedError"
    fn if_platform_supports(self) -> Result<M, IsloationLevelNotSupportedError>;
}

#[derive(Error, Debug)]
#[error("isolation level is not supported by the underlying platform")]
pub struct IsloationLevelNotSupportedError {}

/// Transaction isolation levels.
///
/// Each level controls how much one transaction can see changes made by other transactions.
/// Higher levels prevent more read anomalies, but they can reduce performance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IsolationLevel {
    /// A transaction can read data that another transaction has not yet committed.
    ///
    /// This level does not block dirty reads. You can see uncommitted changes from other transactions.
    /// Possible anomalies: dirty reads, non-repeatable reads, phantom reads.
    ReadUncommitted,

    /// A transaction can only read data that another transaction has committed.
    ///
    /// This level blocks dirty reads, but a row you read twice can still change if another transaction commits an update between the two reads.
    /// Possible anomalies: non-repeatable reads, phantom reads.
    ReadCommitted,

    /// A transaction sees the same snapshot of data for its entire lifetime.
    ///
    /// This level blocks dirty reads and non-repeatable reads. If you read a row twice, you get the same values both times.
    /// Possible anomalies: phantom reads.
    RepeatableRead,

    /// Transactions run as if they execute one after another, not at the same time.
    ///
    /// This level blocks all common read anomalies. It gives the strongest consistency, but it can increase contention and reduce throughput.
    /// Possible anomalies: none of the common read anomalies.
    Serializable,
}
