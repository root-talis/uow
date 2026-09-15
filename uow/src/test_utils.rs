//! Test utilities for the uow crate.
//!
//! These types are useful for unit testing code that depends on
//! [`TxManager`](crate::TxManager) without requiring a real database.
//!
//! Enable via the `test-utils` feature.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use async_trait::async_trait;
use crate::{IsolationLevel, IsolationLevelSelector, IsloationLevelNotSupportedError, TxManager, UoW};

/// A minimal [`UoW`] implementation for tests.
#[derive(Debug, Clone, Copy, Default)]
pub struct TestUoW;

/// Error type returned by [`TestUoW`] operations.
#[derive(Debug, thiserror::Error)]
#[error("test uow error")]
pub struct TestUoWError;

#[async_trait]
impl UoW for TestUoW {
    type Error = TestUoWError;
}

/// A [`TxManager`] implementation for tests that runs closures inline
/// and tracks transaction outcomes.
///
/// # Tracking
///
/// `TestTxManager` tracks how many transactions were started, committed,
/// and rolled back. This is useful for asserting that your usecase
/// correctly propagates errors to the transaction framework.
///
/// # Example
///
/// ```rust
/// use uow::test_utils::TestTxManager;
/// use uow::TxManager;
///
/// # async fn example() {
/// let tx = TestTxManager::new();
/// let result = tx.transaction(|_uow| async move { (_uow, Ok::<_, ()>(42)) }).await;
/// assert_eq!(result.unwrap(), Ok(42));
/// assert_eq!(tx.transactions_committed(), 1);
/// assert_eq!(tx.transactions_rolled_back(), 0);
/// # }
/// ```
#[derive(Debug, Clone, Default)]
pub struct TestTxManager {
    transactions_started: Arc<AtomicUsize>,
    transactions_committed: Arc<AtomicUsize>,
    transactions_rolled_back: Arc<AtomicUsize>,
}

impl TestTxManager {
    /// Creates a new `TestTxManager` with all counters at zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the number of transactions that were started.
    pub fn transactions_started(&self) -> usize {
        self.transactions_started.load(Ordering::SeqCst)
    }

    /// Returns the number of transactions that committed (callback returned `Ok`).
    pub fn transactions_committed(&self) -> usize {
        self.transactions_committed.load(Ordering::SeqCst)
    }

    /// Returns the number of transactions that rolled back (callback returned `Err`).
    pub fn transactions_rolled_back(&self) -> usize {
        self.transactions_rolled_back.load(Ordering::SeqCst)
    }

    /// Resets the `transactions_started` counter to zero.
    ///
    /// Returns `self` for fluent chaining.
    pub fn reset_transactions_started(self) -> Self {
        self.transactions_started.store(0, Ordering::SeqCst);
        self
    }

    /// Resets the `transactions_committed` counter to zero.
    ///
    /// Returns `self` for fluent chaining.
    pub fn reset_transactions_committed(self) -> Self {
        self.transactions_committed.store(0, Ordering::SeqCst);
        self
    }

    /// Resets the `transactions_rolled_back` counter to zero.
    ///
    /// Returns `self` for fluent chaining.
    pub fn reset_transactions_rolled_back(self) -> Self {
        self.transactions_rolled_back.store(0, Ordering::SeqCst);
        self
    }

    /// Resets all counters to zero.
    ///
    /// Returns `self` for fluent chaining.
    pub fn reset(self) -> Self {
        self.reset_transactions_started()
            .reset_transactions_committed()
            .reset_transactions_rolled_back()
    }
}

#[async_trait]
impl TxManager for TestTxManager {
    type UoW = TestUoW;

    async fn transaction<F, Fut, R, E>(
        &self,
        callback: F,
    ) -> Result<Result<R, E>, crate::TransactionError<Self::UoW>>
    where
        F: FnOnce(Self::UoW) -> Fut + Send,
        Fut: std::future::Future<Output = (Self::UoW, Result<R, E>)> + Send,
        R: Send,
        E: Send,
    {
        self.transactions_started.fetch_add(1, Ordering::SeqCst);
        let (_uow, result) = callback(TestUoW).await;
        match &result {
            Ok(_) => {
                self.transactions_committed.fetch_add(1, Ordering::SeqCst);
            }
            Err(_) => {
                self.transactions_rolled_back.fetch_add(1, Ordering::SeqCst);
            }
        }
        Ok(result)
    }

    fn isolated_at(&self, _level: IsolationLevel) -> impl IsolationLevelSelector<Self> {
        TestIsolationLevelSelector(self.clone())
    }
}

/// An [`IsolationLevelSelector`] for [`TestTxManager`] that always succeeds.
///
/// # Example
///
/// ```rust
/// use uow::test_utils::TestTxManager;
/// use uow::{TxManager, IsolationLevelSelector};
///
/// let tx = TestTxManager::new();
/// let tx_with_isolation = tx.isolated_at(uow::IsolationLevel::Serializable).if_platform_supports().unwrap();
/// ```
#[derive(Debug, Clone)]
pub struct TestIsolationLevelSelector(TestTxManager);

impl IsolationLevelSelector<TestTxManager> for TestIsolationLevelSelector {
    fn or(self, _level: IsolationLevel) -> Self {
        self
    }

    fn if_platform_supports(self) -> Result<TestTxManager, IsloationLevelNotSupportedError> {
        Ok(self.0)
    }
}
