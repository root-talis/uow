use async_trait::async_trait;
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::{
    IsloationLevelNotSupportedError, IsolationLevel, IsolationLevelSelector, TransactionError,
    TxManager, UoW,
};

/// SQLx-specific UoW implementation wrapping a SQLite transaction.
#[derive(Debug)]
pub struct SqliteUoW {
    tx: Transaction<'static, Sqlite>,
}

impl SqliteUoW {
    pub fn new(tx: Transaction<'static, Sqlite>) -> Self {
        Self { tx }
    }

    pub fn tx_mut(&mut self) -> &mut Transaction<'static, Sqlite> {
        &mut self.tx
    }

    async fn commit(self) -> Result<(), sqlx::Error> {
        self.tx.commit().await
    }

    async fn rollback(self) -> Result<(), sqlx::Error> {
        self.tx.rollback().await
    }

    async fn reset_pragma(&mut self) -> Result<(), sqlx::Error> {
        sqlx::query("PRAGMA read_uncommitted = 0")
            .execute(self.tx.as_mut())
            .await?;
        Ok(())
    }
}

#[async_trait]
impl UoW for SqliteUoW {
    type Error = sqlx::Error;
}

/// SQLx/Sqlite-specific UoW factory.
#[derive(Clone)]
pub struct SqliteTxManager {
    pool: SqlitePool,
    isolation_level: Option<IsolationLevel>,
}

impl SqliteTxManager {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            isolation_level: None,
        }
    }
}

impl SqliteTxManager {
    async fn unit_of_work(&self) -> Result<SqliteUoW, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        // PRAGMA values cannot be parameterised in SQLite.
        // The value is always a hard-coded 0 or 1, so injection is impossible.
        let pragma_sql: &'static str =
            if matches!(self.isolation_level, Some(IsolationLevel::ReadUncommitted)) {
                "PRAGMA read_uncommitted = 1"
            } else {
                "PRAGMA read_uncommitted = 0"
            };
        sqlx::query(pragma_sql).execute(tx.as_mut()).await?;
        Ok(SqliteUoW::new(tx))
    }
}

#[async_trait]
impl TxManager for SqliteTxManager {
    type UoW = SqliteUoW;

    async fn transaction<F, Fut, R, E>(
        &self,
        f: F,
    ) -> Result<Result<R, E>, TransactionError<Self::UoW>>
    where
        F: FnOnce(Self::UoW) -> Fut + Send,
        Fut: std::future::Future<Output = (Self::UoW, Result<R, E>)> + Send,
        R: Send,
        E: Send,
    {
        let uow = self
            .unit_of_work()
            .await
            .map_err(TransactionError::BeginFailed)?;
        let (mut uow, r) = f(uow).await;
        match r {
            Ok(ok) => {
                uow.reset_pragma()
                    .await
                    .map_err(TransactionError::CommitFailed)?;
                uow.commit().await.map_err(TransactionError::CommitFailed)?;
                Ok(Ok(ok))
            }
            Err(err) => {
                uow.reset_pragma()
                    .await
                    .map_err(TransactionError::RollbackFailed)?;
                if let Err(e) = uow.rollback().await {
                    return Err(TransactionError::RollbackFailed(e));
                }
                Ok(Err(err))
            }
        }
    }

    fn isolated_at(&self, level: IsolationLevel) -> impl IsolationLevelSelector<Self> {
        SqliteIsolationLevelSelector::new(self.clone(), level)
    }
}

pub struct SqliteIsolationLevelSelector {
    man: SqliteTxManager,
    level: Option<IsolationLevel>,
}

impl SqliteIsolationLevelSelector {
    fn new(man: SqliteTxManager, level: IsolationLevel) -> Self {
        Self {
            man,
            level: Self::if_supported(level),
        }
    }

    fn if_supported(level: IsolationLevel) -> Option<IsolationLevel> {
        match level {
            IsolationLevel::Serializable | IsolationLevel::ReadUncommitted => Some(level),
            _ => None,
        }
    }
}

impl IsolationLevelSelector<SqliteTxManager> for SqliteIsolationLevelSelector {
    fn or(mut self, level: IsolationLevel) -> Self {
        self.level = self.level.or_else(|| Self::if_supported(level));
        self
    }

    fn if_platform_supports(mut self) -> Result<SqliteTxManager, IsloationLevelNotSupportedError> {
        if self.level.is_none() {
            return Err(IsloationLevelNotSupportedError {});
        }

        self.man.isolation_level = self.level;
        Ok(self.man)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction;

    // ------------------------------------------------------------------
    // Unit tests (no database required)
    // ------------------------------------------------------------------
    mod unit {
        use super::*;

        /// TS-U01-SQLITE: `SqliteIsolationLevelSelector::if_supported` returns `Some` for
        /// `Serializable` and `ReadUncommitted`.
        ///
        /// ## What is being tested
        /// The invariant that only SQLite-supported isolation levels are accepted by the selector.
        ///
        /// ## Testing strategy
        /// Call `if_supported` directly with each supported variant and assert `Some`.
        #[test]
        fn if_supported_accepts_serializable_and_read_uncommitted() {
            assert_eq!(
                SqliteIsolationLevelSelector::if_supported(IsolationLevel::Serializable),
                Some(IsolationLevel::Serializable)
            );
            assert_eq!(
                SqliteIsolationLevelSelector::if_supported(IsolationLevel::ReadUncommitted),
                Some(IsolationLevel::ReadUncommitted)
            );
        }

        /// TS-U02-SQLITE: `SqliteIsolationLevelSelector::if_supported` returns `None` for
        /// `ReadCommitted` and `RepeatableRead`.
        ///
        /// ## What is being tested
        /// The invariant that unsupported isolation levels are rejected at the selector level.
        ///
        /// ## Testing strategy
        /// Call `if_supported` directly with each unsupported variant and assert `None`.
        #[test]
        fn if_supported_rejects_read_committed_and_repeatable_read() {
            assert_eq!(
                SqliteIsolationLevelSelector::if_supported(IsolationLevel::ReadCommitted),
                None
            );
            assert_eq!(
                SqliteIsolationLevelSelector::if_supported(IsolationLevel::RepeatableRead),
                None
            );
        }

        /// TS-U03-SQLITE: `SqliteIsolationLevelSelector::or` chains fallbacks correctly.
        ///
        /// ## What is being tested
        /// The fallback logic: when the first requested level is unsupported the selector tries
        /// the next one; when the first is supported it is kept and later levels are ignored.
        ///
        /// ## Testing strategy
        /// Build selectors with (unsupported, supported) and (supported, anything) pairs,
        /// then call `if_platform_supports` and inspect the resulting manager's level.
        #[tokio::test]
        async fn or_chains_fallbacks() {
            let man = dummy_manager().await;
            let result =
                SqliteIsolationLevelSelector::new(man.clone(), IsolationLevel::ReadCommitted)
                    .or(IsolationLevel::Serializable)
                    .if_platform_supports();
            assert_eq!(
                result.unwrap().isolation_level,
                Some(IsolationLevel::Serializable)
            );

            let result2 = SqliteIsolationLevelSelector::new(man, IsolationLevel::Serializable)
                .or(IsolationLevel::ReadUncommitted)
                .if_platform_supports();
            assert_eq!(
                result2.unwrap().isolation_level,
                Some(IsolationLevel::Serializable)
            );
        }

        /// TS-U04-SQLITE: `SqliteIsolationLevelSelector::if_platform_supports` returns `Ok(manager)`
        /// when the level is `Some`.
        ///
        /// ## What is being tested
        /// Selector finalisation: a valid configuration produces a manager whose
        /// `isolation_level` field is set.
        ///
        /// ## Testing strategy
        /// Create a selector with `Serializable`, call `if_platform_supports`, assert `Ok`
        /// and that the resulting manager carries `Some(Serializable)`.
        #[tokio::test]
        async fn if_platform_supports_ok_when_level_present() {
            let man = dummy_manager().await;
            let result = SqliteIsolationLevelSelector::new(man, IsolationLevel::Serializable)
                .if_platform_supports();
            assert!(result.is_ok());
            assert_eq!(
                result.unwrap().isolation_level,
                Some(IsolationLevel::Serializable)
            );
        }

        /// TS-U05-SQLITE: `SqliteIsolationLevelSelector::if_platform_supports` returns
        /// `Err(IsloationLevelNotSupportedError)` when the level is `None`.
        ///
        /// ## What is being tested
        /// The error path: if none of the requested isolation levels are supported by SQLite,
        /// the selector returns an error.
        ///
        /// ## Testing strategy
        /// Create a selector using only unsupported levels and assert `Err`.
        #[tokio::test]
        async fn if_platform_supports_err_when_level_none() {
            let man = dummy_manager().await;
            let result = SqliteIsolationLevelSelector::new(man, IsolationLevel::ReadCommitted)
                .or(IsolationLevel::RepeatableRead)
                .if_platform_supports();
            assert!(result.is_err());
        }

        /// TS-U06-SQLITE: `TransactionError::into_inner` extracts the underlying error.
        ///
        /// ## What is being tested
        /// The utility method that discards the `BeginFailed`/`CommitFailed`/`RollbackFailed`
        /// wrapper and returns the original platform error.
        ///
        /// ## Testing strategy
        /// Construct all three variants of `TransactionError` with a dummy `sqlx::Error`,
        /// call `into_inner`, and assert the extracted error matches the original.
        #[test]
        fn transaction_error_into_inner_extracts_source() {
            let inner = sqlx::Error::RowNotFound;
            let err_begin = TransactionError::<SqliteUoW>::BeginFailed(inner);
            let err_commit = TransactionError::<SqliteUoW>::CommitFailed(sqlx::Error::RowNotFound);
            let err_rollback =
                TransactionError::<SqliteUoW>::RollbackFailed(sqlx::Error::RowNotFound);

            assert!(matches!(err_begin.into_inner(), sqlx::Error::RowNotFound));
            assert!(matches!(err_commit.into_inner(), sqlx::Error::RowNotFound));
            assert!(matches!(
                err_rollback.into_inner(),
                sqlx::Error::RowNotFound
            ));
        }

        /// TS-U07-SQLITE: `SqliteTxManager::isolated_at` seeds the selector with the requested level.
        ///
        /// ## What is being tested
        /// The API contract of `isolated_at`: the returned selector must remember the
        /// originally requested isolation level.
        ///
        /// ## Testing strategy
        /// Call `isolated_at(Serializable)` on a manager and verify the selector resolves
        /// to `Some(Serializable)` through `if_platform_supports`.
        #[tokio::test]
        async fn isolated_at_seeds_selector() {
            let man = dummy_manager().await;
            let result = man
                .isolated_at(IsolationLevel::Serializable)
                .if_platform_supports();
            assert_eq!(
                result.unwrap().isolation_level,
                Some(IsolationLevel::Serializable)
            );
        }
    }

    // ------------------------------------------------------------------
    // Integration tests — method API (require an in-memory SQLite database)
    // ------------------------------------------------------------------
    mod integration_method {
        use super::*;
        use sqlx::sqlite::SqlitePoolOptions;

        async fn in_memory_pool() -> SqlitePool {
            SqlitePoolOptions::new()
                .max_connections(5)
                .connect("sqlite::memory:")
                .await
                .expect("in-memory pool")
        }

        /// TS-01-SQLITE: `SqliteTxManager::transaction` commits and persists data.
        ///
        /// ## What is being tested
        /// The `transaction` method API: when the closure returns `Ok`, changes are committed
        /// and visible to subsequent connections.
        ///
        /// ## Testing strategy
        /// Call `transaction` with a closure that creates a table and inserts a row,
        /// assert the outer result is `Ok(Ok(()))`, then query the same database via
        /// the pool and assert the row exists.
        #[tokio::test]
        async fn ts_01_commits_and_persists_data() {
            let pool = in_memory_pool().await;
            let man = SqliteTxManager::new(pool.clone());

            let result = man
                .transaction(|mut uow| async move {
                    sqlx::query("CREATE TABLE t (id INTEGER PRIMARY KEY)")
                        .execute(uow.tx_mut().as_mut())
                        .await
                        .unwrap();
                    sqlx::query("INSERT INTO t (id) VALUES (42)")
                        .execute(uow.tx_mut().as_mut())
                        .await
                        .unwrap();
                    (uow, Ok::<(), ()>(()))
                })
                .await;

            assert!(matches!(result, Ok(Ok(()))));

            let row: (i64,) = sqlx::query_as("SELECT id FROM t")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(row.0, 42);
        }

        /// TS-02-SQLITE: `SqliteTxManager::transaction` rolls back when the closure returns `Err`.
        ///
        /// ## What is being tested
        /// The `transaction` method API automatically rolls back on failure.
        ///
        /// ## Testing strategy
        /// Call `transaction` with a closure that creates a table and then returns `Err`,
        /// assert the outer result is `Ok(Err(...))`, then verify the table does not exist.
        #[tokio::test]
        async fn ts_02_rolls_back_on_err() {
            let pool = in_memory_pool().await;
            let man = SqliteTxManager::new(pool.clone());

            let result = man
                .transaction(|mut uow| async move {
                    sqlx::query("CREATE TABLE t (id INTEGER PRIMARY KEY)")
                        .execute(uow.tx_mut().as_mut())
                        .await
                        .unwrap();
                    (uow, Err::<(), &str>("intentional failure"))
                })
                .await;

            assert!(matches!(result, Ok(Err("intentional failure"))));

            let row: (i64,) = sqlx::query_as("SELECT count(*) FROM sqlite_master WHERE name = 't'")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(row.0, 0);
        }

        /// TS-03-SQLITE: `SqliteTxManager::transaction` commits successfully even when the
        /// closure does nothing.
        ///
        /// ## What is being tested
        /// The minimal success path: beginning a transaction and returning `Ok(())`
        /// produces `Ok(Ok(()))`.
        ///
        /// ## Testing strategy
        /// Call `transaction` with a no-op closure that returns `Ok(())`, then assert
        /// the outer result is `Ok(Ok(()))`.
        #[tokio::test]
        async fn ts_03_begins_and_commits_empty() {
            let pool = in_memory_pool().await;
            let man = SqliteTxManager::new(pool);

            let result = man
                .transaction(|uow| async move { (uow, Ok::<(), ()>(())) })
                .await;

            assert!(matches!(result, Ok(Ok(()))));
        }

        /// TS-04-SQLITE: `SqliteTxManager::transaction` returns `TransactionError::BeginFailed`
        /// when the pool is closed.
        ///
        /// ## What is being tested
        /// The error path: a failure to begin the transaction is propagated as
        /// `TransactionError::BeginFailed`.
        ///
        /// ## Testing strategy
        /// Close the pool before calling `transaction`, then assert the result is
        /// `Err(TransactionError::BeginFailed(...))`.
        #[tokio::test]
        async fn ts_04_begin_failed_when_pool_closed() {
            let pool = in_memory_pool().await;
            let man = SqliteTxManager::new(pool.clone());
            pool.close().await;

            let result = man
                .transaction(|uow| async move { (uow, Ok::<(), ()>(())) })
                .await;

            assert!(matches!(result, Err(TransactionError::BeginFailed(_))));
        }

        /// TS-05-SQLITE: `Serializable` isolation level is propagated from
        /// `SqliteTxManager` into the SQLite transaction.
        ///
        /// ## What is being tested
        /// When a manager is explicitly configured with `Serializable`, the resulting
        /// UoW must keep `PRAGMA read_uncommitted = 0` (the SQLite default).
        ///
        /// ## Testing strategy
        /// Configure a manager with `Serializable`, call `transaction` with a
        /// closure that queries `PRAGMA read_uncommitted` and returns `Ok(row.0)`,
        /// then assert the outer result is `Ok(Ok(0))`.
        #[tokio::test]
        async fn ts_05_serializable_isolation_propagated() {
            let pool = in_memory_pool().await;
            let man = SqliteTxManager::new(pool)
                .isolated_at(IsolationLevel::Serializable)
                .if_platform_supports()
                .unwrap();

            let result = man
                .transaction(|mut uow| async move {
                    let row: (i64,) = sqlx::query_as("PRAGMA read_uncommitted")
                        .fetch_one(uow.tx_mut().as_mut())
                        .await
                        .unwrap();
                    (uow, Ok::<i64, ()>(row.0))
                })
                .await;

            assert!(matches!(result, Ok(Ok(0))));
        }

        /// TS-06-SQLITE: `ReadUncommitted` isolation level is propagated from
        /// `SqliteTxManager` into the SQLite transaction.
        ///
        /// ## What is being tested
        /// The propagation chain from `TxManager.isolation_level` into the SQLite
        /// transaction state.  When a manager is configured with
        /// `ReadUncommitted`, the internal helper must issue
        /// `PRAGMA read_uncommitted = 1` before handing the transaction to the caller.
        ///
        /// ## Testing strategy
        /// Configure a manager with `ReadUncommitted`, call `transaction` with a
        /// closure that queries `PRAGMA read_uncommitted` and returns `Ok(row.0)`,
        /// then assert the outer result is `Ok(Ok(1))`.
        #[tokio::test]
        async fn ts_06_read_uncommitted_isolation_propagated() {
            let pool = in_memory_pool().await;
            let man = SqliteTxManager::new(pool)
                .isolated_at(IsolationLevel::ReadUncommitted)
                .if_platform_supports()
                .unwrap();

            let result = man
                .transaction(|mut uow| async move {
                    let row: (i64,) = sqlx::query_as("PRAGMA read_uncommitted")
                        .fetch_one(uow.tx_mut().as_mut())
                        .await
                        .unwrap();
                    (uow, Ok::<i64, ()>(row.0))
                })
                .await;

            assert!(matches!(result, Ok(Ok(1))));
        }

        /// TS-07-SQLITE: A `ReadUncommitted` transaction commits and the same physical
        /// connection is reused by a `Serializable` transaction.
        ///
        /// ## What is being tested
        /// Connection hygiene: the isolation level pragma must be reset when the
        /// transaction ends, so the next transaction on the same connection starts
        /// with a clean state.
        ///
        /// ## Testing strategy
        /// 1. Create a pool with `max_connections(1)`.
        /// 2. Call `transaction` on manager A (`ReadUncommitted`), query pragma
        ///    and assert `1`, return `Ok(())`.
        /// 3. Call `transaction` on manager B (`Serializable`), query pragma and assert `0`.
        #[tokio::test]
        async fn ts_07_isolation_resets_after_commit_on_single_connection() {
            let pool = SqlitePoolOptions::new()
                .max_connections(1)
                .connect("sqlite::memory:")
                .await
                .expect("single-connection pool");

            let man_a = SqliteTxManager::new(pool.clone())
                .isolated_at(IsolationLevel::ReadUncommitted)
                .if_platform_supports()
                .unwrap();

            let result_a = man_a
                .transaction(|mut uow| async move {
                    let row: (i64,) = sqlx::query_as("PRAGMA read_uncommitted")
                        .fetch_one(uow.tx_mut().as_mut())
                        .await
                        .unwrap();
                    assert_eq!(row.0, 1);
                    (uow, Ok::<(), ()>(()))
                })
                .await;
            assert!(matches!(result_a, Ok(Ok(()))));

            let man_b = SqliteTxManager::new(pool)
                .isolated_at(IsolationLevel::Serializable)
                .if_platform_supports()
                .unwrap();

            let result_b = man_b
                .transaction(|mut uow| async move {
                    let row: (i64,) = sqlx::query_as("PRAGMA read_uncommitted")
                        .fetch_one(uow.tx_mut().as_mut())
                        .await
                        .unwrap();
                    assert_eq!(row.0, 0);
                    (uow, Ok::<(), ()>(()))
                })
                .await;
            assert!(matches!(result_b, Ok(Ok(()))));
        }

        /// TS-08-SQLITE: A `ReadUncommitted` transaction rolls back and the same physical
        /// connection is reused by a `Serializable` transaction.
        ///
        /// ## What is being tested
        /// Connection hygiene after rollback: the isolation level pragma must be reset
        /// even when the transaction is rolled back, so the next transaction on the same
        /// connection starts with a clean state.
        ///
        /// ## Testing strategy
        /// 1. Create a pool with `max_connections(1)`.
        /// 2. Call `transaction` on manager A (`ReadUncommitted`), query pragma
        ///    and assert `1`, return `Err("rollback me")`.
        /// 3. Assert the first result is `Ok(Err("rollback me"))`.
        /// 4. Call `transaction` on manager B (`Serializable`), query pragma and assert `0`.
        #[tokio::test]
        async fn ts_08_isolation_resets_after_rollback_on_single_connection() {
            let pool = SqlitePoolOptions::new()
                .max_connections(1)
                .connect("sqlite::memory:")
                .await
                .expect("single-connection pool");

            let man_a = SqliteTxManager::new(pool.clone())
                .isolated_at(IsolationLevel::ReadUncommitted)
                .if_platform_supports()
                .unwrap();

            let result_a = man_a
                .transaction(|mut uow| async move {
                    let row: (i64,) = sqlx::query_as("PRAGMA read_uncommitted")
                        .fetch_one(uow.tx_mut().as_mut())
                        .await
                        .unwrap();
                    assert_eq!(row.0, 1);
                    (uow, Err::<(), &str>("rollback me"))
                })
                .await;
            assert!(matches!(result_a, Ok(Err("rollback me"))));

            let man_b = SqliteTxManager::new(pool)
                .isolated_at(IsolationLevel::Serializable)
                .if_platform_supports()
                .unwrap();

            let result_b = man_b
                .transaction(|mut uow| async move {
                    let row: (i64,) = sqlx::query_as("PRAGMA read_uncommitted")
                        .fetch_one(uow.tx_mut().as_mut())
                        .await
                        .unwrap();
                    assert_eq!(row.0, 0);
                    (uow, Ok::<(), ()>(()))
                })
                .await;
            assert!(matches!(result_b, Ok(Ok(()))));
        }

        /// TS-09-SQLITE: `SqliteTxManager::transaction` returns `TransactionError::CommitFailed`
        /// when the inner `UoW::commit` fails.
        ///
        /// ## What is being tested
        /// The `transaction` method's error-path: if the closure returns `Ok` but the
        /// subsequent commit fails, the failure is wrapped in
        /// `TransactionError::CommitFailed` rather than being silently dropped.
        ///
        /// ## Testing strategy
        /// Force commit to fail by ending the underlying transaction early.  Inside
        /// the closure we execute `ROLLBACK`, which terminates the transaction.
        /// Returning `Ok` from the closure causes `transaction` to attempt commit;
        /// because the transaction is already ended, sqlx returns an error that is
        /// propagated as `TransactionError::CommitFailed`.
        #[tokio::test]
        async fn ts_09_propagates_commit_failure() {
            let pool = in_memory_pool().await;
            let man = SqliteTxManager::new(pool);

            let result = man
                .transaction(|mut uow| async move {
                    sqlx::query("ROLLBACK")
                        .execute(uow.tx_mut().as_mut())
                        .await
                        .unwrap();
                    (uow, Ok::<(), ()>(()))
                })
                .await;

            assert!(matches!(result, Err(TransactionError::CommitFailed(_))));
        }

        /// TS-10-SQLITE: `SqliteTxManager::transaction` returns `TransactionError::RollbackFailed`
        /// when the inner `UoW::rollback` fails.
        ///
        /// ## What is being tested
        /// The `transaction` method's error-path: if the closure returns `Err` and the
        /// subsequent rollback fails, the failure is wrapped in
        /// `TransactionError::RollbackFailed` rather than being silently dropped.
        ///
        /// ## Testing strategy
        /// Force rollback to fail by ending the underlying transaction early.  Inside
        /// the closure we execute `ROLLBACK`, which terminates the transaction.
        /// Returning `Err` from the closure causes `transaction` to attempt rollback;
        /// because the transaction is already ended, sqlx returns an error that is
        /// propagated as `TransactionError::RollbackFailed`.
        #[tokio::test]
        async fn ts_10_propagates_rollback_failure() {
            let pool = in_memory_pool().await;
            let man = SqliteTxManager::new(pool);

            let result = man
                .transaction(|mut uow| async move {
                    sqlx::query("ROLLBACK")
                        .execute(uow.tx_mut().as_mut())
                        .await
                        .unwrap();
                    (uow, Err::<(), &str>("intentional"))
                })
                .await;

            assert!(matches!(result, Err(TransactionError::RollbackFailed(_))));
        }

        /// TS-11-SQLITE: Fallback isolation level chain resolves end-to-end.
        ///
        /// ## What is being tested
        /// The full selector flow: chaining unsupported levels followed by a supported
        /// one produces a working manager that can run transactions.
        ///
        /// ## Testing strategy
        /// Build a manager via `isolated_at(RepeatableRead).or(Serializable).if_platform_supports()`,
        /// run a transaction that creates a table, and assert it commits successfully.
        #[tokio::test]
        async fn ts_11_fallback_chain_end_to_end() {
            let pool = in_memory_pool().await;
            let man = SqliteTxManager::new(pool)
                .isolated_at(IsolationLevel::RepeatableRead)
                .or(IsolationLevel::Serializable)
                .if_platform_supports()
                .unwrap();

            let result = man
                .transaction(|mut uow| async move {
                    sqlx::query("CREATE TABLE t (id INTEGER PRIMARY KEY)")
                        .execute(uow.tx_mut().as_mut())
                        .await
                        .unwrap();
                    (uow, Ok::<(), ()>(()))
                })
                .await;

            assert!(matches!(result, Ok(Ok(()))));
        }

        /// TS-12-SQLITE: `isolated_at(RepeatableRead).if_platform_supports()` fails on SQLite.
        ///
        /// ## What is being tested
        /// The platform constraint: `RepeatableRead` is unsupported and the selector
        /// must return `Err(IsloationLevelNotSupportedError)`.
        ///
        /// ## Testing strategy
        /// Call the selector directly with an unsupported level and assert `Err`.
        #[tokio::test]
        async fn ts_12_isolated_at_unsupported_level_fails() {
            let man = SqliteTxManager {
                pool: sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap(),
                isolation_level: None,
            };
            let result = man
                .isolated_at(IsolationLevel::RepeatableRead)
                .if_platform_supports();
            assert!(result.is_err());
        }
    }

    // ------------------------------------------------------------------
    // Integration tests — macro API (require an in-memory SQLite database)
    // ------------------------------------------------------------------
    mod integration_macro {
        use super::*;
        use sqlx::sqlite::SqlitePoolOptions;

        async fn in_memory_pool() -> SqlitePool {
            SqlitePoolOptions::new()
                .max_connections(5)
                .connect("sqlite::memory:")
                .await
                .expect("in-memory pool")
        }

        /// TS-01-SQLITE: `transaction!` commits and persists data.
        ///
        /// ## What is being tested
        /// The `transaction!` macro API: when the body returns `Ok`, changes are committed
        /// and visible to subsequent connections.
        ///
        /// ## Testing strategy
        /// Use `transaction!` with `async { ... }` to create a table and insert a row,
        /// assert the outer result is `Ok(Ok(()))`, then query the same database via
        /// the pool and assert the row exists.
        #[tokio::test]
        async fn ts_01_commits_and_persists_data() {
            let pool = in_memory_pool().await;
            let man = SqliteTxManager::new(pool.clone());

            let result = transaction!(man, |mut uow| async {
                sqlx::query("CREATE TABLE t (id INTEGER PRIMARY KEY)")
                    .execute(uow.tx_mut().as_mut())
                    .await
                    .unwrap();
                sqlx::query("INSERT INTO t (id) VALUES (42)")
                    .execute(uow.tx_mut().as_mut())
                    .await
                    .unwrap();
                Ok::<(), ()>(())
            })
            .await;

            assert!(matches!(result, Ok(Ok(()))));

            let row: (i64,) = sqlx::query_as("SELECT id FROM t")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(row.0, 42);
        }

        /// TS-02-SQLITE: `transaction!` rolls back when the body returns `Err`.
        ///
        /// ## What is being tested
        /// The `transaction!` macro API automatically rolls back on failure.
        ///
        /// ## Testing strategy
        /// Use `transaction!` with `async { ... }` to create a table and return `Err`,
        /// assert the outer result is `Ok(Err(...))`, then verify the table does not exist.
        #[tokio::test]
        async fn ts_02_rolls_back_on_err() {
            let pool = in_memory_pool().await;
            let man = SqliteTxManager::new(pool.clone());

            let result = transaction!(man, |mut uow| async {
                sqlx::query("CREATE TABLE t (id INTEGER PRIMARY KEY)")
                    .execute(uow.tx_mut().as_mut())
                    .await
                    .unwrap();
                Err::<(), &str>("intentional failure")
            })
            .await;

            assert!(matches!(result, Ok(Err("intentional failure"))));

            let row: (i64,) = sqlx::query_as("SELECT count(*) FROM sqlite_master WHERE name = 't'")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(row.0, 0);
        }

        /// TS-03-SQLITE: `transaction!` commits successfully even when the body does nothing.
        ///
        /// ## What is being tested
        /// The minimal success path: beginning a transaction via the macro and returning
        /// `Ok(())` produces `Ok(Ok(()))`.
        ///
        /// ## Testing strategy
        /// Use `transaction!` with a no-op body that returns `Ok(())`, then assert
        /// the outer result is `Ok(Ok(()))`.
        #[tokio::test]
        async fn ts_03_begins_and_commits_empty() {
            let pool = in_memory_pool().await;
            let man = SqliteTxManager::new(pool);

            let result = transaction!(man, |mut uow| async {
                // Use uow to suppress unused_mut warning; this is a no-op test.
                let _ = &uow;
                Ok::<(), ()>(())
            })
            .await;

            assert!(matches!(result, Ok(Ok(()))));
        }

        /// TS-04-SQLITE: `transaction!` returns `TransactionError::BeginFailed`
        /// when the pool is closed.
        ///
        /// ## What is being tested
        /// The error path: a failure to begin the transaction is propagated as
        /// `TransactionError::BeginFailed`.
        ///
        /// ## Testing strategy
        /// Close the pool before calling `transaction!`, then assert the result is
        /// `Err(TransactionError::BeginFailed(...))`.
        #[tokio::test]
        async fn ts_04_begin_failed_when_pool_closed() {
            let pool = in_memory_pool().await;
            let man = SqliteTxManager::new(pool.clone());
            pool.close().await;

            let result = transaction!(man, |mut uow| async {
                let _ = &uow;
                Ok::<(), ()>(())
            })
            .await;

            assert!(matches!(result, Err(TransactionError::BeginFailed(_))));
        }

        /// TS-05-SQLITE: `Serializable` isolation level is propagated through the
        /// `transaction!` macro into the SQLite transaction.
        ///
        /// ## What is being tested
        /// When a manager is explicitly configured with `Serializable`, the resulting
        /// UoW must keep `PRAGMA read_uncommitted = 0` (the SQLite default).
        ///
        /// ## Testing strategy
        /// Configure a manager with `Serializable`, use `transaction!` with a
        /// body that queries `PRAGMA read_uncommitted` and returns `Ok(row.0)`,
        /// then assert the outer result is `Ok(Ok(0))`.
        #[tokio::test]
        async fn ts_05_serializable_isolation_propagated() {
            let pool = in_memory_pool().await;
            let man = SqliteTxManager::new(pool)
                .isolated_at(IsolationLevel::Serializable)
                .if_platform_supports()
                .unwrap();

            let result = transaction!(man, |mut uow| async {
                let row: (i64,) = sqlx::query_as("PRAGMA read_uncommitted")
                    .fetch_one(uow.tx_mut().as_mut())
                    .await
                    .unwrap();
                Ok::<i64, ()>(row.0)
            })
            .await;

            assert!(matches!(result, Ok(Ok(0))));
        }

        /// TS-06-SQLITE: `ReadUncommitted` isolation level is propagated through the
        /// `transaction!` macro into the SQLite transaction.
        ///
        /// ## What is being tested
        /// The propagation chain from `TxManager.isolation_level` into the SQLite
        /// transaction state.  When a manager is configured with
        /// `ReadUncommitted`, the internal helper must issue
        /// `PRAGMA read_uncommitted = 1` before handing the transaction to the caller.
        ///
        /// ## Testing strategy
        /// Configure a manager with `ReadUncommitted`, use `transaction!` with a
        /// body that queries `PRAGMA read_uncommitted` and returns `Ok(row.0)`,
        /// then assert the outer result is `Ok(Ok(1))`.
        #[tokio::test]
        async fn ts_06_read_uncommitted_isolation_propagated() {
            let pool = in_memory_pool().await;
            let man = SqliteTxManager::new(pool)
                .isolated_at(IsolationLevel::ReadUncommitted)
                .if_platform_supports()
                .unwrap();

            let result = transaction!(man, |mut uow| async {
                let row: (i64,) = sqlx::query_as("PRAGMA read_uncommitted")
                    .fetch_one(uow.tx_mut().as_mut())
                    .await
                    .unwrap();
                Ok::<i64, ()>(row.0)
            })
            .await;

            assert!(matches!(result, Ok(Ok(1))));
        }

        /// TS-07-SQLITE: A `ReadUncommitted` transaction commits via `transaction!` and the same
        /// physical connection is reused by a `Serializable` transaction.
        ///
        /// ## What is being tested
        /// Connection hygiene: the isolation level pragma must be reset when the
        /// transaction ends, so the next transaction on the same connection starts
        /// with a clean state.
        ///
        /// ## Testing strategy
        /// 1. Create a pool with `max_connections(1)`.
        /// 2. Use `transaction!` on manager A (`ReadUncommitted`), query pragma
        ///    and assert `1`, return `Ok(())`.
        /// 3. Use `transaction!` on manager B (`Serializable`), query pragma and assert `0`.
        #[tokio::test]
        async fn ts_07_isolation_resets_after_commit_on_single_connection() {
            let pool = SqlitePoolOptions::new()
                .max_connections(1)
                .connect("sqlite::memory:")
                .await
                .expect("single-connection pool");

            let man_a = SqliteTxManager::new(pool.clone())
                .isolated_at(IsolationLevel::ReadUncommitted)
                .if_platform_supports()
                .unwrap();

            let result_a = transaction!(man_a, |mut uow| async {
                let row: (i64,) = sqlx::query_as("PRAGMA read_uncommitted")
                    .fetch_one(uow.tx_mut().as_mut())
                    .await
                    .unwrap();
                assert_eq!(row.0, 1);
                Ok::<(), ()>(())
            })
            .await;
            assert!(matches!(result_a, Ok(Ok(()))));

            let man_b = SqliteTxManager::new(pool)
                .isolated_at(IsolationLevel::Serializable)
                .if_platform_supports()
                .unwrap();

            let result_b = transaction!(man_b, |mut uow| async {
                let row: (i64,) = sqlx::query_as("PRAGMA read_uncommitted")
                    .fetch_one(uow.tx_mut().as_mut())
                    .await
                    .unwrap();
                assert_eq!(row.0, 0);
                Ok::<(), ()>(())
            })
            .await;
            assert!(matches!(result_b, Ok(Ok(()))));
        }

        /// TS-08-SQLITE: A `ReadUncommitted` transaction rolls back via `transaction!` and the same
        /// physical connection is reused by a `Serializable` transaction.
        ///
        /// ## What is being tested
        /// Connection hygiene after rollback: the isolation level pragma must be reset
        /// even when the transaction is rolled back, so the next transaction on the same
        /// connection starts with a clean state.
        ///
        /// ## Testing strategy
        /// 1. Create a pool with `max_connections(1)`.
        /// 2. Use `transaction!` on manager A (`ReadUncommitted`), query pragma
        ///    and assert `1`, return `Err("rollback me")`.
        /// 3. Assert the first result is `Ok(Err("rollback me"))`.
        /// 4. Use `transaction!` on manager B (`Serializable`), query pragma and assert `0`.
        #[tokio::test]
        async fn ts_08_isolation_resets_after_rollback_on_single_connection() {
            let pool = SqlitePoolOptions::new()
                .max_connections(1)
                .connect("sqlite::memory:")
                .await
                .expect("single-connection pool");

            let man_a = SqliteTxManager::new(pool.clone())
                .isolated_at(IsolationLevel::ReadUncommitted)
                .if_platform_supports()
                .unwrap();

            let result_a = transaction!(man_a, |mut uow| async {
                let row: (i64,) = sqlx::query_as("PRAGMA read_uncommitted")
                    .fetch_one(uow.tx_mut().as_mut())
                    .await
                    .unwrap();
                assert_eq!(row.0, 1);
                Err::<(), &str>("rollback me")
            })
            .await;
            assert!(matches!(result_a, Ok(Err("rollback me"))));

            let man_b = SqliteTxManager::new(pool)
                .isolated_at(IsolationLevel::Serializable)
                .if_platform_supports()
                .unwrap();

            let result_b = transaction!(man_b, |mut uow| async {
                let row: (i64,) = sqlx::query_as("PRAGMA read_uncommitted")
                    .fetch_one(uow.tx_mut().as_mut())
                    .await
                    .unwrap();
                assert_eq!(row.0, 0);
                Ok::<(), ()>(())
            })
            .await;
            assert!(matches!(result_b, Ok(Ok(()))));
        }

        /// TS-09-SQLITE: `transaction!` returns `TransactionError::CommitFailed`
        /// when the inner `UoW::commit` fails.
        ///
        /// ## What is being tested
        /// The `transaction!` macro's error-path: if the body returns `Ok` but the
        /// subsequent commit fails, the failure is wrapped in
        /// `TransactionError::CommitFailed` rather than being silently dropped.
        ///
        /// ## Testing strategy
        /// Force commit to fail by ending the underlying transaction early.  Inside
        /// the body we execute `ROLLBACK`, which terminates the transaction.
        /// Returning `Ok` from the body causes `transaction!` to attempt commit;
        /// because the transaction is already ended, sqlx returns an error that is
        /// propagated as `TransactionError::CommitFailed`.
        #[tokio::test]
        async fn ts_09_propagates_commit_failure() {
            let pool = in_memory_pool().await;
            let man = SqliteTxManager::new(pool);

            let result = transaction!(man, |mut uow| async {
                sqlx::query("ROLLBACK")
                    .execute(uow.tx_mut().as_mut())
                    .await
                    .unwrap();
                Ok::<(), ()>(())
            })
            .await;

            assert!(matches!(result, Err(TransactionError::CommitFailed(_))));
        }

        /// TS-10-SQLITE: `transaction!` returns `TransactionError::RollbackFailed`
        /// when the inner `UoW::rollback` fails.
        ///
        /// ## What is being tested
        /// The `transaction!` macro's error-path: if the body returns `Err` and the
        /// subsequent rollback fails, the failure is wrapped in
        /// `TransactionError::RollbackFailed` rather than being silently dropped.
        ///
        /// ## Testing strategy
        /// Force rollback to fail by ending the underlying transaction early.  Inside
        /// the body we execute `ROLLBACK`, which terminates the transaction.
        /// Returning `Err` from the body causes `transaction!` to attempt rollback;
        /// because the transaction is already ended, sqlx returns an error that is
        /// propagated as `TransactionError::RollbackFailed`.
        #[tokio::test]
        async fn ts_10_propagates_rollback_failure() {
            let pool = in_memory_pool().await;
            let man = SqliteTxManager::new(pool);

            let result = transaction!(man, |mut uow| async {
                sqlx::query("ROLLBACK")
                    .execute(uow.tx_mut().as_mut())
                    .await
                    .unwrap();
                Err::<(), &str>("intentional")
            })
            .await;

            assert!(matches!(result, Err(TransactionError::RollbackFailed(_))));
        }

        /// TS-11-SQLITE: Fallback isolation level chain resolves end-to-end via `transaction!`.
        ///
        /// ## What is being tested
        /// The full selector flow: chaining unsupported levels followed by a supported
        /// one produces a working manager that can run transactions through the macro.
        ///
        /// ## Testing strategy
        /// Build a manager via `isolated_at(RepeatableRead).or(Serializable).if_platform_supports()`,
        /// use `transaction!` to create a table, and assert it commits successfully.
        #[tokio::test]
        async fn ts_11_fallback_chain_end_to_end() {
            let pool = in_memory_pool().await;
            let man = SqliteTxManager::new(pool)
                .isolated_at(IsolationLevel::RepeatableRead)
                .or(IsolationLevel::Serializable)
                .if_platform_supports()
                .unwrap();

            let result = transaction!(man, |mut uow| async {
                sqlx::query("CREATE TABLE t (id INTEGER PRIMARY KEY)")
                    .execute(uow.tx_mut().as_mut())
                    .await
                    .unwrap();
                Ok::<(), ()>(())
            })
            .await;

            assert!(matches!(result, Ok(Ok(()))));
        }

        /// TS-13-SQLITE: `transaction!` with `async move` commits when the body returns `Ok`.
        ///
        /// ## What is being tested
        /// The macro's `move` variant correctly delegates to `TxManager::transaction`
        /// and commits on success, capturing variables by move.
        ///
        /// ## Testing strategy
        /// Use `transaction!` with `async move { ... }` to create a table and return
        /// `Ok(())`. Assert the outer result is `Ok(Ok(()))`, then verify the table
        /// exists by querying `sqlite_master`.
        #[tokio::test]
        async fn ts_13_async_move_variant_commits() {
            let pool = in_memory_pool().await;
            let man = SqliteTxManager::new(pool.clone());

            let table_name = "t".to_string();

            let result = transaction!(man, |mut uow| async move {
                sqlx::query("CREATE TABLE t (id INTEGER PRIMARY KEY)")
                    .execute(uow.tx_mut().as_mut())
                    .await
                    .unwrap();
                let row: (String,) = sqlx::query_as("SELECT ?1")
                    .bind(table_name)
                    .fetch_one(uow.tx_mut().as_mut())
                    .await
                    .unwrap();
                assert_eq!(row.0, "t");
                Ok::<(), ()>(())
            })
            .await;

            assert!(matches!(result, Ok(Ok(()))));

            let row: (i64,) = sqlx::query_as("SELECT count(*) FROM sqlite_master WHERE name = 't'")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(row.0, 1);
        }
    }

    // ------------------------------------------------------------------
    // Test doubles and helpers (encountered after all scenarios)
    // ------------------------------------------------------------------

    /// Creates a dummy `SqliteTxManager` for unit tests that never touch the database.
    /// Uses an in-memory pool created inside an async runtime.
    async fn dummy_manager() -> SqliteTxManager {
        SqliteTxManager {
            pool: SqlitePool::connect("sqlite::memory:")
                .await
                .expect("in-memory pool"),
            isolation_level: None,
        }
    }
}
