use async_trait::async_trait;
use thiserror::Error;

/// Trait for a Unit of Work (transaction wrapper).
/// Allows different transaction backends to be plugged in.
#[async_trait]
pub trait UoW: Send {
    /// The error type returned by this UoW implementation.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Commit the transaction.
    async fn commit(self) -> Result<(), Self::Error>;
}

/// Trait for a Transaction manager.
/// Allows different data source backends to be plugged in.
#[async_trait]
pub trait TxManager: Clone + Send + Sync {
    /// The type of Unit of Work this manager creates.
    type UoW: UoW;

    /// Create a new Unit of Work instance.
    async fn unit_of_work(&self) -> Result<Self::UoW, <Self::UoW as UoW>::Error>;

    /// Execute a closure within a Unit of Work, automatically committing on success
    /// or rolling back on failure.
    ///
    /// The closure takes ownership of the Unit of Work and returns it along with the result,
    /// avoiding lifetime issues with async closures.
    async fn transaction<F, Fut, R, E>(&self, callback: F) -> Result<Result<R, E>, <Self::UoW as UoW>::Error>
    where
        F: FnOnce(Self::UoW) -> Fut + Send,
        Fut: std::future::Future<Output = (Self::UoW, Result<TxResult<R>, TxResult<E>>)> + Send,
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
    fn isolated_at(&self, level: IsolationLevel) -> impl IsolationLevelSelector<Self>;
}

pub enum TxResult<T> {
    Commit(T),
    Rollback(T)
}

/// Trait for an Isolation level selector.
/// 
/// Sets transaction isolation level for a new Transaction manager.
/// Ensures that the underlying platform supports requested isolation level.
/// Supports configuring a chain of fallbacks.
pub trait IsolationLevelSelector<M: TxManager> {
    /// Specifies the next preferrable isolation level if previous specified levels
    /// are not supported by the plaform.
    /// 
    /// Used to configure a chain of fallbacks.
    fn or(self, level: IsolationLevel) -> Self;

    /// Returns a new Transaction manager with configured isolation level.
    /// 
    /// If none of specified isolation levels are supporetd by the underlying platform,
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
#[derive(Clone, Debug)]
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
