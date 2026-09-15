use async_trait::async_trait;
use sqlx::{PgPool, Postgres, Transaction};

use crate::{
    IsloationLevelNotSupportedError, IsolationLevel, IsolationLevelSelector, TransactionError,
    TxManager, UoW,
};

/// SQLx-specific UoW implementation wrapping a PostgreSQL transaction.
#[derive(Debug)]
pub struct PostgresUoW {
    tx: Transaction<'static, Postgres>,
}

impl PostgresUoW {
    pub fn new(tx: Transaction<'static, Postgres>) -> Self {
        Self { tx }
    }

    pub fn tx_mut(&mut self) -> &mut Transaction<'static, Postgres> {
        &mut self.tx
    }

    async fn commit(self) -> Result<(), sqlx::Error> {
        self.tx.commit().await
    }

    async fn rollback(self) -> Result<(), sqlx::Error> {
        self.tx.rollback().await
    }
}

#[async_trait]
impl UoW for PostgresUoW {
    type Error = sqlx::Error;
}

/// SQLx/PostgreSQL-specific UoW factory.
#[derive(Clone)]
pub struct PostgresTxManager {
    pool: PgPool,
    isolation_level: Option<IsolationLevel>,
}

impl PostgresTxManager {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            isolation_level: None,
        }
    }
}

impl PostgresTxManager {
    async fn unit_of_work(&self) -> Result<PostgresUoW, sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        if let Some(ref level) = self.isolation_level {
            // PostgreSQL isolation levels cannot be parameterised in SET TRANSACTION.
            // The SQL string is always a hard-coded constant derived from the enum,
            // so injection is impossible.
            let iso_sql: &'static str = match level {
                IsolationLevel::ReadUncommitted => {
                    "SET TRANSACTION ISOLATION LEVEL READ UNCOMMITTED"
                }
                IsolationLevel::ReadCommitted => "SET TRANSACTION ISOLATION LEVEL READ COMMITTED",
                IsolationLevel::RepeatableRead => "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ",
                IsolationLevel::Serializable => "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE",
            };
            sqlx::query(iso_sql).execute(tx.as_mut()).await?;
        }

        Ok(PostgresUoW::new(tx))
    }
}

#[async_trait]
impl TxManager for PostgresTxManager {
    type UoW = PostgresUoW;

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
        let (uow, r) = f(uow).await;
        match r {
            Ok(ok) => {
                uow.commit().await.map_err(TransactionError::CommitFailed)?;
                Ok(Ok(ok))
            }
            Err(err) => {
                if let Err(e) = uow.rollback().await {
                    return Err(TransactionError::RollbackFailed(e));
                }
                Ok(Err(err))
            }
        }
    }

    fn isolated_at(&self, level: IsolationLevel) -> impl IsolationLevelSelector<Self> {
        PostgresIsolationLevelSelector::new(self.clone(), level)
    }
}

pub struct PostgresIsolationLevelSelector {
    man: PostgresTxManager,
    level: Option<IsolationLevel>,
}

impl PostgresIsolationLevelSelector {
    fn new(man: PostgresTxManager, level: IsolationLevel) -> Self {
        Self {
            man,
            level: Self::if_supported(level),
        }
    }

    fn if_supported(level: IsolationLevel) -> Option<IsolationLevel> {
        // PostgreSQL accepts all four standard isolation levels.
        // Note: READ UNCOMMITTED is accepted by PostgreSQL but internally
        // treated as READ COMMITTED (it does not allow dirty reads).
        match level {
            IsolationLevel::ReadUncommitted
            | IsolationLevel::ReadCommitted
            | IsolationLevel::RepeatableRead
            | IsolationLevel::Serializable => Some(level),
        }
    }
}

impl IsolationLevelSelector<PostgresTxManager> for PostgresIsolationLevelSelector {
    fn or(mut self, level: IsolationLevel) -> Self {
        self.level = self.level.or_else(|| Self::if_supported(level));
        self
    }

    fn if_platform_supports(
        mut self,
    ) -> Result<PostgresTxManager, IsloationLevelNotSupportedError> {
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

    // ------------------------------------------------------------------
    // Unit tests (no database required)
    // ------------------------------------------------------------------
    mod unit {
        use super::*;

        /// TS-U01-PGSQL: `PostgresIsolationLevelSelector::if_supported` accepts all four levels.
        ///
        /// ## What is being tested
        /// The invariant that all standard isolation levels are accepted by the PostgreSQL selector.
        ///
        /// ## Testing strategy
        /// Call `if_supported` directly with each variant and assert `Some`.
        #[test]
        fn if_supported_accepts_all_four_levels() {
            assert_eq!(
                PostgresIsolationLevelSelector::if_supported(IsolationLevel::ReadUncommitted),
                Some(IsolationLevel::ReadUncommitted)
            );
            assert_eq!(
                PostgresIsolationLevelSelector::if_supported(IsolationLevel::ReadCommitted),
                Some(IsolationLevel::ReadCommitted)
            );
            assert_eq!(
                PostgresIsolationLevelSelector::if_supported(IsolationLevel::RepeatableRead),
                Some(IsolationLevel::RepeatableRead)
            );
            assert_eq!(
                PostgresIsolationLevelSelector::if_supported(IsolationLevel::Serializable),
                Some(IsolationLevel::Serializable)
            );
        }

        /// TS-U03-PGSQL: `PostgresIsolationLevelSelector::or` chains fallbacks correctly.
        ///
        /// ## What is being tested
        /// The fallback logic: when the first requested level is supported it is kept and
        /// later levels are ignored.
        ///
        /// ## Testing strategy
        /// Build selectors with (supported, supported) pairs and verify the first level wins.
        #[tokio::test]
        async fn or_chains_fallbacks() {
            let man = dummy_manager();
            let result =
                PostgresIsolationLevelSelector::new(man.clone(), IsolationLevel::Serializable)
                    .or(IsolationLevel::ReadCommitted)
                    .if_platform_supports();
            assert_eq!(
                result.unwrap().isolation_level,
                Some(IsolationLevel::Serializable)
            );

            let result2 = PostgresIsolationLevelSelector::new(man, IsolationLevel::ReadUncommitted)
                .or(IsolationLevel::RepeatableRead)
                .if_platform_supports();
            assert_eq!(
                result2.unwrap().isolation_level,
                Some(IsolationLevel::ReadUncommitted)
            );
        }

        /// TS-U05-PGSQL: `PostgresIsolationLevelSelector::if_platform_supports` returns
        /// `Err(IsloationLevelNotSupportedError)` when the level is `None`.
        ///
        /// ## What is being tested
        /// The error path: if the selector's level is `None`, it returns an error.
        ///
        /// ## Testing strategy
        /// Create a selector with no level and assert `Err`.
        #[tokio::test]
        async fn if_platform_supports_err_when_level_none() {
            let man = dummy_manager();
            let mut selector =
                PostgresIsolationLevelSelector::new(man, IsolationLevel::Serializable);
            selector.level = None;
            let result = selector.if_platform_supports();
            assert!(result.is_err());
        }

        /// TS-U06-PGSQL: `TransactionError::into_inner` extracts the underlying error.
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
            let err_begin = TransactionError::<PostgresUoW>::BeginFailed(inner);
            let err_commit =
                TransactionError::<PostgresUoW>::CommitFailed(sqlx::Error::RowNotFound);
            let err_rollback =
                TransactionError::<PostgresUoW>::RollbackFailed(sqlx::Error::RowNotFound);

            assert!(matches!(err_begin.into_inner(), sqlx::Error::RowNotFound));
            assert!(matches!(err_commit.into_inner(), sqlx::Error::RowNotFound));
            assert!(matches!(
                err_rollback.into_inner(),
                sqlx::Error::RowNotFound
            ));
        }

        /// TS-U07-PGSQL: `PostgresTxManager::isolated_at` seeds the selector with the requested level.
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
            let man = dummy_manager();
            let result = man
                .isolated_at(IsolationLevel::Serializable)
                .if_platform_supports();
            assert_eq!(
                result.unwrap().isolation_level,
                Some(IsolationLevel::Serializable)
            );
        }

        /// TS-U08-PGSQL: `PostgresIsolationLevelSelector::or` chains three or more fallbacks.
        ///
        /// ## What is being tested
        /// The fallback logic supports chaining more than one `or()` call. Only the
        /// first supported level should be kept and subsequent levels ignored.
        ///
        /// ## Testing strategy
        /// Build a selector with `.or(A).or(B).or(C)` where all levels are supported,
        /// and verify the first level wins.
        #[tokio::test]
        async fn or_chains_three_fallbacks() {
            let man = dummy_manager();
            let result = PostgresIsolationLevelSelector::new(man, IsolationLevel::Serializable)
                .or(IsolationLevel::RepeatableRead)
                .or(IsolationLevel::ReadCommitted)
                .if_platform_supports();
            assert_eq!(
                result.unwrap().isolation_level,
                Some(IsolationLevel::Serializable)
            );
        }

        /// TS-U09-PGSQL: `PostgresIsolationLevelSelector::or` uses fallback when current level is None.
        ///
        /// ## What is being tested
        /// The `or()` method's fallback path: when the selector's current level is `None`,
        /// the first `or()` fallback should be used.
        ///
        /// ## Testing strategy
        /// Manually set `selector.level = None`, then call `.or(ReadCommitted).if_platform_supports()`
        /// and assert the resulting manager carries `Some(ReadCommitted)`.
        ///
        /// ## Note
        /// PostgreSQL accepts all four standard isolation levels, so this None state can only
        /// be reached synthetically in tests or through future platform-specific rejection logic.
        #[tokio::test]
        async fn or_uses_fallback_when_first_unsupported() {
            let man = dummy_manager();
            let mut selector =
                PostgresIsolationLevelSelector::new(man, IsolationLevel::Serializable);
            selector.level = None;
            let result = selector
                .or(IsolationLevel::ReadCommitted)
                .if_platform_supports();
            assert_eq!(
                result.unwrap().isolation_level,
                Some(IsolationLevel::ReadCommitted)
            );
        }
    }

    // ------------------------------------------------------------------
    // Integration tests — method API (require a real PostgreSQL database)
    // ------------------------------------------------------------------
    mod integration_method {
        use super::*;
        use sqlx::postgres::PgPoolOptions;
        use testcontainers::ContainerAsync;
        use testcontainers::runners::AsyncRunner;
        use testcontainers_modules::postgres::Postgres;
        use tokio::sync::OnceCell;

        struct TestDb {
            #[allow(dead_code)]
            container: ContainerAsync<Postgres>,
            url: String,
        }

        static TEST_DB: OnceCell<TestDb> = OnceCell::const_new();

        async fn test_db() -> &'static TestDb {
            TEST_DB
                .get_or_init(|| async {
                    let container = Postgres::default().start().await
                        .expect("Failed to start PostgreSQL test container. Is Docker running?");
                    let host = container.get_host().await
                        .expect("Failed to get test container host");
                    let port = container.get_host_port_ipv4(5432).await
                        .expect("Failed to get test container port 5432");
                    let url = format!("postgres://postgres:postgres@{}:{}/postgres", host, port);
                    TestDb { container, url }
                })
                .await
        }

        async fn fresh_pool() -> PgPool {
            let url = &test_db().await.url;
            PgPoolOptions::new()
                .max_connections(5)
                .acquire_timeout(std::time::Duration::from_secs(60))
                .connect(url)
                .await
                .unwrap()
        }

        /// TS-01-PGSQL: `PostgresTxManager::transaction` commits and persists data.
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
            let pool = fresh_pool().await;
            let man = PostgresTxManager::new(pool.clone());

            let result = man
                .transaction(|mut uow| async move {
                    sqlx::query("CREATE TABLE test_table (id SERIAL PRIMARY KEY, value TEXT)")
                        .execute(uow.tx_mut().as_mut())
                        .await
                        .unwrap();
                    sqlx::query("INSERT INTO test_table (value) VALUES ('hello')")
                        .execute(uow.tx_mut().as_mut())
                        .await
                        .unwrap();
                    (uow, Ok::<(), ()>(()))
                })
                .await;

            assert!(matches!(result, Ok(Ok(()))));

            let row: (String,) = sqlx::query_as("SELECT value FROM test_table")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(row.0, "hello");

            sqlx::query("DROP TABLE IF EXISTS test_table")
                .execute(&pool)
                .await
                .unwrap();
        }

        /// TS-02-PGSQL: `PostgresTxManager::transaction` rolls back when the closure returns `Err`.
        ///
        /// ## What is being tested
        /// The `transaction` method API automatically rolls back on failure.
        ///
        /// ## Testing strategy
        /// Call `transaction` with a closure that creates a table and then returns `Err`,
        /// assert the outer result is `Ok(Err(...))`, then verify the table does not exist.
        #[tokio::test]
        async fn ts_02_rolls_back_on_err() {
            let pool = fresh_pool().await;
            let man = PostgresTxManager::new(pool.clone());

            let result = man
                .transaction(|mut uow| async move {
                    sqlx::query("CREATE TABLE ts02_test (id SERIAL PRIMARY KEY)")
                        .execute(uow.tx_mut().as_mut())
                        .await
                        .unwrap();
                    (uow, Err::<(), &str>("intentional failure"))
                })
                .await;

            assert!(matches!(result, Ok(Err("intentional failure"))));

            let row: (i64,) = sqlx::query_as(
                "SELECT count(*) FROM information_schema.tables WHERE table_name = 'ts02_test'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(row.0, 0);

            // Defensive cleanup: if rollback failed, the table might persist.
            sqlx::query("DROP TABLE IF EXISTS ts02_test")
                .execute(&pool)
                .await
                .unwrap();
        }

        /// TS-03-PGSQL: `PostgresTxManager::transaction` commits successfully even when the
        /// closure does nothing.
        ///
        /// ## What is being tested
        /// The minimal success path: beginning a transaction and returning `Ok(())`
        /// produces `Ok(Ok(()))`.
        ///
        /// TS-04-PGSQL: `PostgresTxManager::transaction` returns `TransactionError::BeginFailed`
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
            let pool = fresh_pool().await;
            let man = PostgresTxManager::new(pool.clone());
            pool.close().await;

            let result = man
                .transaction(|uow| async move { (uow, Ok::<(), ()>(())) })
                .await;

            assert!(matches!(result, Err(TransactionError::BeginFailed(_))));
        }

        /// TS-05-PGSQL: `Serializable` isolation level is propagated from
        /// `PostgresTxManager` into the PostgreSQL transaction.
        ///
        /// ## What is being tested
        /// When a manager is explicitly configured with `Serializable`, the resulting
        /// UoW must run under `SERIALIZABLE` isolation.
        ///
        /// ## Testing strategy
        /// Configure a manager with `Serializable`, call `transaction` with a
        /// closure that queries `SHOW transaction_isolation` and returns `Ok(row.0)`,
        /// then assert the outer result is `Ok(Ok("serializable"))`.
        #[tokio::test]
        async fn ts_05_serializable_isolation_propagated() {
            let pool = fresh_pool().await;
            let man = PostgresTxManager::new(pool)
                .isolated_at(IsolationLevel::Serializable)
                .if_platform_supports()
                .unwrap();

            let result = man
                .transaction(|mut uow| async move {
                    let row: (String,) = sqlx::query_as("SHOW transaction_isolation")
                        .fetch_one(uow.tx_mut().as_mut())
                        .await
                        .unwrap();
                    (uow, Ok::<String, ()>(row.0))
                })
                .await;

            assert!(
                matches!(result, Ok(Ok(ref s)) if s == "serializable"),
                "result: {:?}",
                result
            );
        }

        /// TS-06-PGSQL: `ReadCommitted` isolation level is propagated from
        /// `PostgresTxManager` into the PostgreSQL transaction.
        ///
        /// ## What is being tested
        /// When a manager is explicitly configured with `ReadCommitted`, the resulting
        /// UoW must run under `READ COMMITTED` isolation.
        ///
        /// ## Testing strategy
        /// Configure a manager with `ReadCommitted`, call `transaction` with a
        /// closure that queries `SHOW transaction_isolation` and returns `Ok(row.0)`,
        /// then assert the outer result is `Ok(Ok("read committed"))`.
        #[tokio::test]
        async fn ts_06_read_committed_isolation_propagated() {
            let pool = fresh_pool().await;
            let man = PostgresTxManager::new(pool)
                .isolated_at(IsolationLevel::ReadCommitted)
                .if_platform_supports()
                .unwrap();

            let result = man
                .transaction(|mut uow| async move {
                    let row: (String,) = sqlx::query_as("SHOW transaction_isolation")
                        .fetch_one(uow.tx_mut().as_mut())
                        .await
                        .unwrap();
                    (uow, Ok::<String, ()>(row.0))
                })
                .await;

            assert!(
                matches!(result, Ok(Ok(ref s)) if s == "read committed"),
                "result: {:?}",
                result
            );
        }

        /// TS-07-PGSQL: A `ReadCommitted` transaction commits and the same physical
        /// connection is reused by a `Serializable` transaction.
        ///
        /// ## What is being tested
        /// Connection hygiene: the isolation level must be reset (or explicitly set)
        /// when each transaction begins, so the next transaction on the same connection
        /// starts with the correct isolation level.
        ///
        /// ## Testing strategy
        /// 1. Create a pool with `max_connections(1)`.
        /// 2. Call `transaction` on manager A (`ReadCommitted`), query isolation
        ///    and assert `"read committed"`, return `Ok(())`.
        /// 3. Call `transaction` on manager B (`Serializable`), query isolation and assert `"serializable"`.
        #[tokio::test]
        async fn ts_07_isolation_resets_after_commit_on_single_connection() {
            let pool = PgPoolOptions::new()
                .max_connections(1)
                .acquire_timeout(std::time::Duration::from_secs(60))
                .connect(&test_db().await.url)
                .await
                .unwrap();

            let man_a = PostgresTxManager::new(pool.clone())
                .isolated_at(IsolationLevel::ReadCommitted)
                .if_platform_supports()
                .unwrap();

            let result_a = man_a
                .transaction(|mut uow| async move {
                    let row: (String,) = sqlx::query_as("SHOW transaction_isolation")
                        .fetch_one(uow.tx_mut().as_mut())
                        .await
                        .unwrap();
                    assert_eq!(row.0, "read committed");
                    (uow, Ok::<(), ()>(()))
                })
                .await;
            assert!(matches!(result_a, Ok(Ok(()))));

            let man_b = PostgresTxManager::new(pool)
                .isolated_at(IsolationLevel::Serializable)
                .if_platform_supports()
                .unwrap();

            let result_b = man_b
                .transaction(|mut uow| async move {
                    let row: (String,) = sqlx::query_as("SHOW transaction_isolation")
                        .fetch_one(uow.tx_mut().as_mut())
                        .await
                        .unwrap();
                    assert_eq!(row.0, "serializable");
                    (uow, Ok::<(), ()>(()))
                })
                .await;
            assert!(matches!(result_b, Ok(Ok(()))));
        }

        /// TS-08-PGSQL: A `ReadCommitted` transaction rolls back and the same physical
        /// connection is reused by a `Serializable` transaction.
        ///
        /// ## What is being tested
        /// Connection hygiene after rollback: the isolation level must be set correctly
        /// even when the transaction is rolled back, so the next transaction on the same
        /// connection starts with the correct level.
        ///
        /// ## Testing strategy
        /// 1. Create a pool with `max_connections(1)`.
        /// 2. Call `transaction` on manager A (`ReadCommitted`), query isolation
        ///    and assert `"read committed"`, return `Err("rollback me")`.
        /// 3. Assert the first result is `Ok(Err("rollback me"))`.
        /// 4. Call `transaction` on manager B (`Serializable`), query isolation and assert `"serializable"`.
        #[tokio::test]
        async fn ts_08_isolation_resets_after_rollback_on_single_connection() {
            let pool = PgPoolOptions::new()
                .max_connections(1)
                .acquire_timeout(std::time::Duration::from_secs(60))
                .connect(&test_db().await.url)
                .await
                .unwrap();

            let man_a = PostgresTxManager::new(pool.clone())
                .isolated_at(IsolationLevel::ReadCommitted)
                .if_platform_supports()
                .unwrap();

            let result_a = man_a
                .transaction(|mut uow| async move {
                    let row: (String,) = sqlx::query_as("SHOW transaction_isolation")
                        .fetch_one(uow.tx_mut().as_mut())
                        .await
                        .unwrap();
                    assert_eq!(row.0, "read committed");
                    (uow, Err::<(), &str>("rollback me"))
                })
                .await;
            assert!(matches!(result_a, Ok(Err("rollback me"))));

            let man_b = PostgresTxManager::new(pool)
                .isolated_at(IsolationLevel::Serializable)
                .if_platform_supports()
                .unwrap();

            let result_b = man_b
                .transaction(|mut uow| async move {
                    let row: (String,) = sqlx::query_as("SHOW transaction_isolation")
                        .fetch_one(uow.tx_mut().as_mut())
                        .await
                        .unwrap();
                    assert_eq!(row.0, "serializable");
                    (uow, Ok::<(), ()>(()))
                })
                .await;
            assert!(matches!(result_b, Ok(Ok(()))));
        }

        /// TS-09-PGSQL: `PostgresTxManager::transaction` returns `TransactionError::CommitFailed`
        /// when the inner `UoW::commit` fails.
        ///
        /// ## What is being tested
        /// The `transaction` method's error-path: if the closure returns `Ok` but the
        /// subsequent commit fails, the failure is wrapped in
        /// `TransactionError::CommitFailed` rather than being silently dropped.
        ///
        /// ## Testing strategy
        /// Force commit to fail using a deferred constraint violation.  Inside the
        /// closure we create a table with a self-referential deferred foreign key,
        /// insert a row that violates the constraint, and return `Ok`.  PostgreSQL
        /// checks deferred constraints at commit time, so the commit fails and the
        /// error is propagated as `TransactionError::CommitFailed`.
        #[tokio::test]
        async fn ts_09_propagates_commit_failure() {
            let pool = fresh_pool().await;
            let man = PostgresTxManager::new(pool.clone());

            let result = man
                .transaction(|mut uow| async move {
                    sqlx::query(
                        "CREATE TABLE ts09_test (
                            id INT PRIMARY KEY,
                            parent_id INT REFERENCES ts09_test(id) DEFERRABLE INITIALLY DEFERRED
                        )",
                    )
                    .execute(uow.tx_mut().as_mut())
                    .await
                    .unwrap();
                    sqlx::query("INSERT INTO ts09_test (id, parent_id) VALUES (1, 999)")
                        .execute(uow.tx_mut().as_mut())
                        .await
                        .unwrap();
                    (uow, Ok::<(), ()>(()))
                })
                .await;

            assert!(matches!(result, Err(TransactionError::CommitFailed(_))));

            sqlx::query("DROP TABLE IF EXISTS ts09_test")
                .execute(&pool)
                .await
                .unwrap();
        }

        /// TS-10-PGSQL: `PostgresTxManager::transaction` returns `TransactionError::RollbackFailed`
        /// when the inner `UoW::rollback` fails.
        ///
        /// ## What is being tested
        /// The `transaction` method's error-path: if the closure returns `Err` and the
        /// subsequent rollback fails, the failure is wrapped in
        /// `TransactionError::RollbackFailed` rather than being silently dropped.
        ///
        /// ## Testing strategy
        /// Force rollback to fail by terminating the backend connection from inside
        /// the closure.  We query `pg_backend_pid()`, open a second connection to
        /// the same database, and call `pg_terminate_backend` on ourselves.  The
        /// connection is severed, so when `transaction` tries to rollback it receives
        /// a database error that is propagated as `TransactionError::RollbackFailed`.
        #[tokio::test]
        async fn ts_10_propagates_rollback_failure() {
            let pool = fresh_pool().await;
            let url = test_db().await.url.clone();
            let man = PostgresTxManager::new(pool);

            let result = man
                .transaction(|mut uow| async move {
                    let row: (i32,) = sqlx::query_as("SELECT pg_backend_pid()")
                        .fetch_one(uow.tx_mut().as_mut())
                        .await
                        .unwrap();
                    let my_pid = row.0;

                    // Open a second connection and terminate our own backend.
                    let pool2 = PgPoolOptions::new()
                        .max_connections(1)
                        .acquire_timeout(std::time::Duration::from_secs(60))
                        .connect(&url)
                        .await
                        .unwrap();
                    sqlx::query("SELECT pg_terminate_backend($1)")
                        .bind(my_pid)
                        .execute(&pool2)
                        .await
                        .unwrap();

                    // Wait until the backend has actually terminated.
                    let start = std::time::Instant::now();
                    loop {
                        match sqlx::query_as::<_, (Option<bool>,)>(
                            "SELECT pg_terminate_backend($1) FROM pg_stat_activity WHERE pid = $1",
                        )
                        .bind(my_pid)
                        .fetch_one(&pool2)
                        .await
                        {
                            Ok((None,)) | Err(sqlx::Error::RowNotFound) => break, // backend no longer exists
                            Ok((Some(_),)) => {}
                            Err(e) => panic!("unexpected error polling backend: {}", e),
                        }
                        if start.elapsed() > std::time::Duration::from_secs(5) {
                            panic!("backend did not terminate in time");
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    }

                    (uow, Err::<(), &str>("intentional"))
                })
                .await;

            assert!(matches!(result, Err(TransactionError::RollbackFailed(_))));
        }

        /// TS-11-PGSQL: Fallback isolation level chain resolves end-to-end.
        ///
        /// ## What is being tested
        /// The full selector flow: chaining levels through `or` and `if_platform_supports`
        /// produces a working manager that can run transactions.
        ///
        /// ## Testing strategy
        /// Build a manager via `isolated_at(ReadUncommitted).or(Serializable).if_platform_supports()`,
        /// run a transaction that creates a table, and assert it commits successfully.
        #[tokio::test]
        async fn ts_11_fallback_chain_end_to_end() {
            let pool = fresh_pool().await;
            let man = PostgresTxManager::new(pool.clone())
                .isolated_at(IsolationLevel::ReadUncommitted)
                .or(IsolationLevel::Serializable)
                .if_platform_supports()
                .unwrap();

            let result = man
                .transaction(|mut uow| async move {
                    sqlx::query("CREATE TABLE ts11_test (id SERIAL PRIMARY KEY)")
                        .execute(uow.tx_mut().as_mut())
                        .await
                        .unwrap();
                    (uow, Ok::<(), ()>(()))
                })
                .await;

            assert!(matches!(result, Ok(Ok(()))));

            sqlx::query("DROP TABLE IF EXISTS ts11_test")
                .execute(&pool)
                .await
                .unwrap();
        }

        /// TS-12-PGSQL: Early `Err` inside transaction body triggers rollback.
        ///
        /// ## What is being tested
        /// When a transaction body returns `Err` before reaching the end, the
        /// transaction rolls back and the error is propagated correctly.
        ///
        /// ## Testing strategy
        /// Call `transaction` with a closure that creates a table, then returns
        /// `Err` early. Assert the outer result is `Ok(Err(...))` and that the
        /// table does not exist.
        #[tokio::test]
        async fn ts_12_early_err_triggers_rollback() {
            let pool = fresh_pool().await;
            let man = PostgresTxManager::new(pool.clone());

            let result = man
                .transaction(|mut uow| async move {
                    sqlx::query("CREATE TABLE ts12_test (id SERIAL PRIMARY KEY)")
                        .execute(uow.tx_mut().as_mut())
                        .await
                        .unwrap();
                    // Early return — should trigger rollback
                    return (uow, Err::<(), &str>("early failure"));
                })
                .await;

            assert!(matches!(result, Ok(Err("early failure"))));

            let row: (i64,) = sqlx::query_as(
                "SELECT count(*) FROM information_schema.tables WHERE table_name = 'ts12_test'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(row.0, 0);

            sqlx::query("DROP TABLE IF EXISTS ts12_test")
                .execute(&pool)
                .await
                .unwrap();
        }

        /// TS-17-PGSQL: `PostgresUoW::tx_mut()` returns a working mutable transaction reference.
        ///
        /// ## What is being tested
        /// `PostgresUoW::tx_mut()` returns a mutable reference that can be used to
        /// execute queries directly via the underlying `sqlx::Transaction`.
        ///
        /// ## Testing strategy
        /// Obtain a `PostgresUoW` via `unit_of_work()`, call `tx_mut()` to get the
        /// transaction reference, execute a query through it, then roll back.
        #[tokio::test]
        async fn ts_17_tx_mut_returns_working_transaction() {
            let pool = fresh_pool().await;
            let man = PostgresTxManager::new(pool);
            let mut uow = man.unit_of_work().await.unwrap();

            let tx = uow.tx_mut();
            sqlx::query("SELECT 1").execute(tx.as_mut()).await.unwrap();

            uow.rollback().await.unwrap();
        }
    }

    // ------------------------------------------------------------------
    // Integration tests — macro API (require a real PostgreSQL database)
    // ------------------------------------------------------------------
    mod integration_macro {
        use super::*;
        use crate::transaction;
        use sqlx::postgres::PgPoolOptions;
        use testcontainers::ContainerAsync;
        use testcontainers::runners::AsyncRunner;
        use testcontainers_modules::postgres::Postgres;
        use tokio::sync::OnceCell;

        struct TestDb {
            #[allow(dead_code)]
            container: ContainerAsync<Postgres>,
            url: String,
        }

        static TEST_DB: OnceCell<TestDb> = OnceCell::const_new();

        async fn test_db() -> &'static TestDb {
            TEST_DB
                .get_or_init(|| async {
                    let container = Postgres::default().start().await
                        .expect("Failed to start PostgreSQL test container. Is Docker running?");
                    let host = container.get_host().await
                        .expect("Failed to get test container host");
                    let port = container.get_host_port_ipv4(5432).await
                        .expect("Failed to get test container port 5432");
                    let url = format!("postgres://postgres:postgres@{}:{}/postgres", host, port);
                    TestDb { container, url }
                })
                .await
        }

        async fn fresh_pool() -> PgPool {
            let url = &test_db().await.url;
            PgPoolOptions::new()
                .max_connections(5)
                .acquire_timeout(std::time::Duration::from_secs(60))
                .connect(url)
                .await
                .unwrap()
        }

        /// TS-01-PGSQL: `transaction!` commits and persists data.
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
            let pool = fresh_pool().await;
            let man = PostgresTxManager::new(pool.clone());

            let result = transaction!(man, |mut uow| async {
                sqlx::query("CREATE TABLE ts01_test (id SERIAL PRIMARY KEY, value TEXT)")
                    .execute(uow.tx_mut().as_mut())
                    .await
                    .unwrap();
                sqlx::query("INSERT INTO ts01_test (value) VALUES ('hello')")
                    .execute(uow.tx_mut().as_mut())
                    .await
                    .unwrap();
                Ok::<(), ()>(())
            })
            .await;

            assert!(matches!(result, Ok(Ok(()))));

            let row: (String,) = sqlx::query_as("SELECT value FROM ts01_test")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(row.0, "hello");

            sqlx::query("DROP TABLE IF EXISTS ts01_test")
                .execute(&pool)
                .await
                .unwrap();
        }

        /// TS-02-PGSQL: `transaction!` rolls back when the body returns `Err`.
        ///
        /// ## What is being tested
        /// The `transaction!` macro API automatically rolls back on failure.
        ///
        /// ## Testing strategy
        /// Use `transaction!` with `async { ... }` to create a table and return `Err`,
        /// assert the outer result is `Ok(Err(...))`, then verify the table does not exist.
        #[tokio::test]
        async fn ts_02_rolls_back_on_err() {
            let pool = fresh_pool().await;
            let man = PostgresTxManager::new(pool.clone());

            let result = transaction!(man, |mut uow| async {
                sqlx::query("CREATE TABLE ts02_test (id SERIAL PRIMARY KEY)")
                    .execute(uow.tx_mut().as_mut())
                    .await
                    .unwrap();
                Err::<(), &str>("intentional failure")
            })
            .await;

            assert!(matches!(result, Ok(Err("intentional failure"))));

            let row: (i64,) = sqlx::query_as(
                "SELECT count(*) FROM information_schema.tables WHERE table_name = 'ts02_test'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(row.0, 0);

            sqlx::query("DROP TABLE IF EXISTS ts02_test")
                .execute(&pool)
                .await
                .unwrap();
        }

        /// TS-04-PGSQL: `transaction!` returns `TransactionError::BeginFailed`
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
            let pool = fresh_pool().await;
            let man = PostgresTxManager::new(pool.clone());
            pool.close().await;

            let result = transaction!(man, |mut uow| async {
                let _ = &uow;
                Ok::<(), ()>(())
            })
            .await;

            assert!(matches!(result, Err(TransactionError::BeginFailed(_))));
        }

        /// TS-05-PGSQL: `Serializable` isolation level is propagated through the
        /// `transaction!` macro into the PostgreSQL transaction.
        ///
        /// ## What is being tested
        /// When a manager is explicitly configured with `Serializable`, the resulting
        /// UoW must run under `SERIALIZABLE` isolation.
        ///
        /// ## Testing strategy
        /// Configure a manager with `Serializable`, use `transaction!` with a
        /// body that queries `SHOW transaction_isolation` and returns `Ok(row.0)`,
        /// then assert the outer result is `Ok(Ok("serializable"))`.
        #[tokio::test]
        async fn ts_05_serializable_isolation_propagated() {
            let pool = fresh_pool().await;
            let man = PostgresTxManager::new(pool)
                .isolated_at(IsolationLevel::Serializable)
                .if_platform_supports()
                .unwrap();

            let result = transaction!(man, |mut uow| async {
                let row: (String,) = sqlx::query_as("SHOW transaction_isolation")
                    .fetch_one(uow.tx_mut().as_mut())
                    .await
                    .unwrap();
                Ok::<String, ()>(row.0)
            })
            .await;

            assert!(
                matches!(result, Ok(Ok(ref s)) if s == "serializable"),
                "result: {:?}",
                result
            );
        }

        /// TS-06-PGSQL: `ReadCommitted` isolation level is propagated through the
        /// `transaction!` macro into the PostgreSQL transaction.
        ///
        /// ## What is being tested
        /// When a manager is explicitly configured with `ReadCommitted`, the resulting
        /// UoW must run under `READ COMMITTED` isolation.
        ///
        /// ## Testing strategy
        /// Configure a manager with `ReadCommitted`, use `transaction!` with a
        /// body that queries `SHOW transaction_isolation` and returns `Ok(row.0)`,
        /// then assert the outer result is `Ok(Ok("read committed"))`.
        #[tokio::test]
        async fn ts_06_read_committed_isolation_propagated() {
            let pool = fresh_pool().await;
            let man = PostgresTxManager::new(pool)
                .isolated_at(IsolationLevel::ReadCommitted)
                .if_platform_supports()
                .unwrap();

            let result = transaction!(man, |mut uow| async {
                let row: (String,) = sqlx::query_as("SHOW transaction_isolation")
                    .fetch_one(uow.tx_mut().as_mut())
                    .await
                    .unwrap();
                Ok::<String, ()>(row.0)
            })
            .await;

            assert!(
                matches!(result, Ok(Ok(ref s)) if s == "read committed"),
                "result: {:?}",
                result
            );
        }

        /// TS-07-PGSQL: A `ReadCommitted` transaction commits via `transaction!` and the same
        /// physical connection is reused by a `Serializable` transaction.
        ///
        /// ## What is being tested
        /// Connection hygiene: the isolation level must be reset (or explicitly set)
        /// when each transaction begins, so the next transaction on the same connection
        /// starts with the correct isolation level.
        ///
        /// ## Testing strategy
        /// 1. Create a pool with `max_connections(1)`.
        /// 2. Use `transaction!` on manager A (`ReadCommitted`), query isolation
        ///    and assert `"read committed"`, return `Ok(())`.
        /// 3. Use `transaction!` on manager B (`Serializable`), query isolation and assert `"serializable"`.
        #[tokio::test]
        async fn ts_07_isolation_resets_after_commit_on_single_connection() {
            let pool = PgPoolOptions::new()
                .max_connections(1)
                .acquire_timeout(std::time::Duration::from_secs(60))
                .connect(&test_db().await.url)
                .await
                .unwrap();

            let man_a = PostgresTxManager::new(pool.clone())
                .isolated_at(IsolationLevel::ReadCommitted)
                .if_platform_supports()
                .unwrap();

            let result_a = transaction!(man_a, |mut uow| async {
                let row: (String,) = sqlx::query_as("SHOW transaction_isolation")
                    .fetch_one(uow.tx_mut().as_mut())
                    .await
                    .unwrap();
                assert_eq!(row.0, "read committed");
                Ok::<(), ()>(())
            })
            .await;
            assert!(matches!(result_a, Ok(Ok(()))));

            let man_b = PostgresTxManager::new(pool)
                .isolated_at(IsolationLevel::Serializable)
                .if_platform_supports()
                .unwrap();

            let result_b = transaction!(man_b, |mut uow| async {
                let row: (String,) = sqlx::query_as("SHOW transaction_isolation")
                    .fetch_one(uow.tx_mut().as_mut())
                    .await
                    .unwrap();
                assert_eq!(row.0, "serializable");
                Ok::<(), ()>(())
            })
            .await;
            assert!(matches!(result_b, Ok(Ok(()))));
        }

        /// TS-08-PGSQL: A `ReadCommitted` transaction rolls back via `transaction!` and the same
        /// physical connection is reused by a `Serializable` transaction.
        ///
        /// ## What is being tested
        /// Connection hygiene after rollback: the isolation level must be set correctly
        /// even when the transaction is rolled back, so the next transaction on the same
        /// connection starts with the correct level.
        ///
        /// ## Testing strategy
        /// 1. Create a pool with `max_connections(1)`.
        /// 2. Use `transaction!` on manager A (`ReadCommitted`), query isolation
        ///    and assert `"read committed"`, return `Err("rollback me")`.
        /// 3. Assert the first result is `Ok(Err("rollback me"))`.
        /// 4. Use `transaction!` on manager B (`Serializable`), query isolation and assert `"serializable"`.
        #[tokio::test]
        async fn ts_08_isolation_resets_after_rollback_on_single_connection() {
            let pool = PgPoolOptions::new()
                .max_connections(1)
                .acquire_timeout(std::time::Duration::from_secs(60))
                .connect(&test_db().await.url)
                .await
                .unwrap();

            let man_a = PostgresTxManager::new(pool.clone())
                .isolated_at(IsolationLevel::ReadCommitted)
                .if_platform_supports()
                .unwrap();

            let result_a = transaction!(man_a, |mut uow| async {
                let row: (String,) = sqlx::query_as("SHOW transaction_isolation")
                    .fetch_one(uow.tx_mut().as_mut())
                    .await
                    .unwrap();
                assert_eq!(row.0, "read committed");
                Err::<(), &str>("rollback me")
            })
            .await;
            assert!(matches!(result_a, Ok(Err("rollback me"))));

            let man_b = PostgresTxManager::new(pool)
                .isolated_at(IsolationLevel::Serializable)
                .if_platform_supports()
                .unwrap();

            let result_b = transaction!(man_b, |mut uow| async {
                let row: (String,) = sqlx::query_as("SHOW transaction_isolation")
                    .fetch_one(uow.tx_mut().as_mut())
                    .await
                    .unwrap();
                assert_eq!(row.0, "serializable");
                Ok::<(), ()>(())
            })
            .await;
            assert!(matches!(result_b, Ok(Ok(()))));
        }

        /// TS-09-PGSQL: `transaction!` returns `TransactionError::CommitFailed`
        /// when the inner `UoW::commit` fails.
        ///
        /// ## What is being tested
        /// The `transaction!` macro's error-path: if the body returns `Ok` but the
        /// subsequent commit fails, the failure is wrapped in
        /// `TransactionError::CommitFailed` rather than being silently dropped.
        ///
        /// ## Testing strategy
        /// Force commit to fail using a deferred constraint violation.  Inside the
        /// body we create a table with a self-referential deferred foreign key,
        /// insert a row that violates the constraint, and return `Ok`.  PostgreSQL
        /// checks deferred constraints at commit time, so the commit fails and the
        /// error is propagated as `TransactionError::CommitFailed`.
        #[tokio::test]
        async fn ts_09_propagates_commit_failure() {
            let pool = fresh_pool().await;
            let man = PostgresTxManager::new(pool.clone());

            let result = transaction!(man, |mut uow| async {
                sqlx::query(
                    "CREATE TABLE ts09_test (
                        id INT PRIMARY KEY,
                        parent_id INT REFERENCES ts09_test(id) DEFERRABLE INITIALLY DEFERRED
                    )",
                )
                .execute(uow.tx_mut().as_mut())
                .await
                .unwrap();
                sqlx::query("INSERT INTO ts09_test (id, parent_id) VALUES (1, 999)")
                    .execute(uow.tx_mut().as_mut())
                    .await
                    .unwrap();
                Ok::<(), ()>(())
            })
            .await;

            assert!(matches!(result, Err(TransactionError::CommitFailed(_))));

            sqlx::query("DROP TABLE IF EXISTS ts09_test")
                .execute(&pool)
                .await
                .unwrap();
        }

        /// TS-10-PGSQL: `transaction!` returns `TransactionError::RollbackFailed`
        /// when the inner `UoW::rollback` fails.
        ///
        /// ## What is being tested
        /// The `transaction!` macro's error-path: if the body returns `Err` and the
        /// subsequent rollback fails, the failure is wrapped in
        /// `TransactionError::RollbackFailed` rather than being silently dropped.
        ///
        /// ## Testing strategy
        /// Force rollback to fail by terminating the backend connection from inside
        /// the body.  We query `pg_backend_pid()`, open a second connection to
        /// the same database, and call `pg_terminate_backend` on ourselves.  The
        /// connection is severed, so when `transaction!` tries to rollback it receives
        /// a database error that is propagated as `TransactionError::RollbackFailed`.
        #[tokio::test]
        async fn ts_10_propagates_rollback_failure() {
            let pool = fresh_pool().await;
            let url = test_db().await.url.clone();
            let man = PostgresTxManager::new(pool);

            let result = transaction!(man, |mut uow| async move {
                let row: (i32,) = sqlx::query_as("SELECT pg_backend_pid()")
                    .fetch_one(uow.tx_mut().as_mut())
                    .await
                    .unwrap();
                let my_pid = row.0;

                // Open a second connection and terminate our own backend.
                let pool2 = PgPoolOptions::new()
                    .max_connections(1)
                    .acquire_timeout(std::time::Duration::from_secs(60))
                    .connect(&url)
                    .await
                    .unwrap();
                sqlx::query("SELECT pg_terminate_backend($1)")
                    .bind(my_pid)
                    .execute(&pool2)
                    .await
                    .unwrap();

                // Wait until the backend has actually terminated.
                let start = std::time::Instant::now();
                loop {
                    match sqlx::query_as::<_, (Option<bool>,)>(
                        "SELECT pg_terminate_backend($1) FROM pg_stat_activity WHERE pid = $1",
                    )
                    .bind(my_pid)
                    .fetch_one(&pool2)
                    .await
                    {
                        Ok((None,)) | Err(sqlx::Error::RowNotFound) => break, // backend no longer exists
                        Ok((Some(_),)) => {}
                        Err(e) => panic!("unexpected error polling backend: {}", e),
                    }
                    if start.elapsed() > std::time::Duration::from_secs(5) {
                        panic!("backend did not terminate in time");
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }

                Err::<(), &str>("intentional")
            })
            .await;

            assert!(matches!(result, Err(TransactionError::RollbackFailed(_))));
        }

        /// TS-11-PGSQL: Fallback isolation level chain resolves end-to-end via `transaction!`.
        ///
        /// ## What is being tested
        /// The full selector flow: chaining levels through `or` and `if_platform_supports`
        /// produces a working manager that can run transactions through the macro.
        ///
        /// ## Testing strategy
        /// Build a manager via `isolated_at(ReadUncommitted).or(Serializable).if_platform_supports()`,
        /// use `transaction!` to create a table, and assert it commits successfully.
        #[tokio::test]
        async fn ts_11_fallback_chain_end_to_end() {
            let pool = fresh_pool().await;
            let man = PostgresTxManager::new(pool.clone())
                .isolated_at(IsolationLevel::ReadUncommitted)
                .or(IsolationLevel::Serializable)
                .if_platform_supports()
                .unwrap();

            let result = transaction!(man, |mut uow| async {
                sqlx::query("CREATE TABLE ts11_test (id SERIAL PRIMARY KEY)")
                    .execute(uow.tx_mut().as_mut())
                    .await
                    .unwrap();
                Ok::<(), ()>(())
            })
            .await;

            assert!(matches!(result, Ok(Ok(()))));

            sqlx::query("DROP TABLE IF EXISTS ts11_test")
                .execute(&pool)
                .await
                .unwrap();
        }

        /// TS-12-PGSQL: Early `Err` inside `transaction!` body triggers rollback.
        ///
        /// ## What is being tested
        /// When a `transaction!` body encounters an early `Err` (via `?` operator),
        /// the macro correctly propagates the error and triggers rollback.
        ///
        /// ## Testing strategy
        /// Use `transaction!` with a body that creates a table, then uses the `?`
        /// operator on a function that returns `Err`. Assert the outer result is
        /// `Ok(Err(...))` and that the table does not exist.
        #[tokio::test]
        async fn ts_12_early_err_triggers_rollback() {
            let pool = fresh_pool().await;
            let man = PostgresTxManager::new(pool.clone());

            fn fallible_op(should_fail: bool) -> Result<(), &'static str> {
                if should_fail {
                    Err("early failure")
                } else {
                    Ok(())
                }
            }

            let result = transaction!(man, |mut uow| async {
                sqlx::query("CREATE TABLE ts12_test (id SERIAL PRIMARY KEY)")
                    .execute(uow.tx_mut().as_mut())
                    .await
                    .unwrap();
                fallible_op(true)?; // Early return triggers rollback
                Ok::<(), &str>(())
            })
            .await;

            assert!(matches!(result, Ok(Err("early failure"))));

            let row: (i64,) = sqlx::query_as(
                "SELECT count(*) FROM information_schema.tables WHERE table_name = 'ts12_test'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(row.0, 0);

            sqlx::query("DROP TABLE IF EXISTS ts12_test")
                .execute(&pool)
                .await
                .unwrap();
        }

        /// TS-13-PGSQL: `transaction!` with `async move` commits when the body returns `Ok`.
        ///
        /// ## What is being tested
        /// The macro's `move` variant correctly delegates to `TxManager::transaction`
        /// and commits on success, capturing variables by move.
        ///
        /// ## Testing strategy
        /// Use `transaction!` with `async move { ... }` to create a table and return
        /// `Ok(())`. Assert the outer result is `Ok(Ok(()))`, then verify the table
        /// exists by querying `information_schema.tables`.
        #[tokio::test]
        async fn ts_13_async_move_variant_commits() {
            let pool = fresh_pool().await;
            let man = PostgresTxManager::new(pool.clone());

            let table_name = "ts13_test".to_string();

            let result = transaction!(man, |mut uow| async move {
                sqlx::query("CREATE TABLE ts13_test (id SERIAL PRIMARY KEY)")
                    .execute(uow.tx_mut().as_mut())
                    .await
                    .unwrap();
                let row: (String,) = sqlx::query_as("SELECT $1")
                    .bind(&table_name)
                    .fetch_one(uow.tx_mut().as_mut())
                    .await
                    .unwrap();
                assert_eq!(row.0, "ts13_test");
                Ok::<(), ()>(())
            })
            .await;

            assert!(matches!(result, Ok(Ok(()))));

            let row: (i64,) = sqlx::query_as(
                "SELECT count(*) FROM information_schema.tables WHERE table_name = 'ts13_test'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(row.0, 1);

            sqlx::query("DROP TABLE IF EXISTS ts13_test")
                .execute(&pool)
                .await
                .unwrap();
        }
    }

    // ------------------------------------------------------------------
    // Integration tests — PostgreSQL-specific behaviour
    // ------------------------------------------------------------------
    mod integration_pgsql_specific {
        use super::*;
        use sqlx::postgres::PgPoolOptions;
        use testcontainers::ContainerAsync;
        use testcontainers::runners::AsyncRunner;
        use testcontainers_modules::postgres::Postgres;
        use tokio::sync::OnceCell;

        struct TestDb {
            #[allow(dead_code)]
            container: ContainerAsync<Postgres>,
            url: String,
        }

        static TEST_DB: OnceCell<TestDb> = OnceCell::const_new();

        async fn test_db() -> &'static TestDb {
            TEST_DB
                .get_or_init(|| async {
                    let container = Postgres::default().start().await
                        .expect("Failed to start PostgreSQL test container. Is Docker running?");
                    let host = container.get_host().await
                        .expect("Failed to get test container host");
                    let port = container.get_host_port_ipv4(5432).await
                        .expect("Failed to get test container port 5432");
                    let url = format!("postgres://postgres:postgres@{}:{}/postgres", host, port);
                    TestDb { container, url }
                })
                .await
        }

        async fn fresh_pool() -> PgPool {
            let url = &test_db().await.url;
            PgPoolOptions::new()
                .max_connections(5)
                .acquire_timeout(std::time::Duration::from_secs(60))
                .connect(url)
                .await
                .unwrap()
        }

        /// TS-14-PGSQL: `RepeatableRead` prevents non-repeatable read.
        ///
        /// ## What is being tested
        /// `RepeatableRead` guarantees a consistent snapshot across multiple `SELECT`s
        /// in the same transaction, even when another transaction commits an update
        /// between the two reads.
        ///
        /// ## Testing strategy
        /// 1. Create a table `ts14_test` with one row `(id=1, value='before')`.
        /// 2. Start transaction A with `RepeatableRead`.
        /// 3. In transaction A, read the row and assert `value='before'`.
        /// 4. From the pool (outside any transaction), update the row to `value='after'` and commit.
        /// 5. In transaction A, read the row again and assert `value='before'` (snapshot isolation).
        /// 6. Commit transaction A.
        /// 7. Drop the table.
        #[tokio::test]
        async fn ts_14_repeatable_read_prevents_non_repeatable_read() {
            let pool = fresh_pool().await;

            // Setup
            sqlx::query("CREATE TABLE ts14_test (id INT PRIMARY KEY, value TEXT)")
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query("INSERT INTO ts14_test (id, value) VALUES (1, 'before')")
                .execute(&pool)
                .await
                .unwrap();

            let man = PostgresTxManager::new(pool.clone())
                .isolated_at(IsolationLevel::RepeatableRead)
                .if_platform_supports()
                .unwrap();

            let pool2 = pool.clone();
            man.transaction(|mut uow| async move {
                // First read inside transaction A
                let row: (String,) = sqlx::query_as("SELECT value FROM ts14_test WHERE id = 1")
                    .fetch_one(uow.tx_mut().as_mut())
                    .await
                    .unwrap();
                assert_eq!(row.0, "before");

                // Concurrent update from outside the transaction
                sqlx::query("UPDATE ts14_test SET value = 'after' WHERE id = 1")
                    .execute(&pool2)
                    .await
                    .unwrap();

                // Second read inside transaction A — should still see the snapshot
                let row: (String,) = sqlx::query_as("SELECT value FROM ts14_test WHERE id = 1")
                    .fetch_one(uow.tx_mut().as_mut())
                    .await
                    .unwrap();
                assert_eq!(row.0, "before");

                (uow, Ok::<(), ()>(()))
            })
            .await
            .unwrap()
            .unwrap();

            // Cleanup
            sqlx::query("DROP TABLE IF EXISTS ts14_test")
                .execute(&pool)
                .await
                .unwrap();
        }

        /// TS-15-PGSQL: `Serializable` detects write-skew anomaly.
        ///
        /// ## What is being tested
        /// PostgreSQL's `Serializable` isolation level detects write-skew anomalies
        /// using SSI (Serializable Snapshot Isolation) and raises `40001`
        /// (serialization_failure).
        ///
        /// ## Testing strategy
        /// 1. Create a table `ts15_test` with two rows representing doctors on call:
        ///    `(id=1, name='alice', on_call=true)` and `(id=2, name='bob', on_call=true)`.
        /// 2. Start transaction A with `Serializable`.
        /// 3. Start transaction B with `Serializable`.
        /// 4. In transaction A, count doctors on call → gets 2. Since count >= 2,
        ///    Alice decides to go off call: `UPDATE ts15_test SET on_call=false WHERE id=1`.
        /// 5. In transaction B, count doctors on call → gets 2. Since count >= 2,
        ///    Bob decides to go off call: `UPDATE ts15_test SET on_call=false WHERE id=2`.
        /// 6. Commit transaction A.
        /// 7. Attempt to commit transaction B → should fail with `serialization_failure`
        ///    (sqlx::Error::Database with code `40001`).
        /// 8. Assert that at least one transaction fails with a serialization error.
        /// 9. Drop the table.
        #[tokio::test]
        async fn ts_15_serializable_detects_write_skew() {
            let pool = fresh_pool().await;

            // Setup
            sqlx::query(
                "CREATE TABLE ts15_test (
                    id INT PRIMARY KEY,
                    name TEXT,
                    on_call BOOLEAN
                )",
            )
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query("INSERT INTO ts15_test (id, name, on_call) VALUES (1, 'alice', true)")
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query("INSERT INTO ts15_test (id, name, on_call) VALUES (2, 'bob', true)")
                .execute(&pool)
                .await
                .unwrap();

            let man_a = PostgresTxManager::new(pool.clone())
                .isolated_at(IsolationLevel::Serializable)
                .if_platform_supports()
                .unwrap();
            let man_b = PostgresTxManager::new(pool.clone())
                .isolated_at(IsolationLevel::Serializable)
                .if_platform_supports()
                .unwrap();

            // Use a barrier to ensure both transactions start concurrently
            let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));
            let barrier_a = barrier.clone();
            let barrier_b = barrier.clone();

            // Transaction A
            let handle_a = tokio::spawn(async move {
                man_a
                    .transaction(|mut uow| async move {
                        let count: (i64,) =
                            sqlx::query_as("SELECT COUNT(*) FROM ts15_test WHERE on_call = true")
                                .fetch_one(uow.tx_mut().as_mut())
                                .await
                                .unwrap();
                        if count.0 >= 2 {
                            sqlx::query("UPDATE ts15_test SET on_call = false WHERE id = 1")
                                .execute(uow.tx_mut().as_mut())
                                .await
                                .unwrap();
                        }
                        barrier_a.wait().await;
                        (uow, Ok::<(), ()>(()))
                    })
                    .await
            });

            // Transaction B
            let handle_b = tokio::spawn(async move {
                man_b
                    .transaction(|mut uow| async move {
                        let count: (i64,) =
                            sqlx::query_as("SELECT COUNT(*) FROM ts15_test WHERE on_call = true")
                                .fetch_one(uow.tx_mut().as_mut())
                                .await
                                .unwrap();
                        if count.0 >= 2 {
                            sqlx::query("UPDATE ts15_test SET on_call = false WHERE id = 2")
                                .execute(uow.tx_mut().as_mut())
                                .await
                                .unwrap();
                        }
                        barrier_b.wait().await;
                        (uow, Ok::<(), ()>(()))
                    })
                    .await
            });

            let (result_a, result_b) = tokio::join!(handle_a, handle_b);
            let result_a = result_a.unwrap();
            let result_b = result_b.unwrap();

            // At least one transaction must fail with serialization_failure (40001)
            let is_serialization_error =
                |result: &Result<Result<(), ()>, TransactionError<PostgresUoW>>| {
                    matches!(
                        result,
                        Err(TransactionError::CommitFailed(sqlx::Error::Database(db_err)))
                            if db_err.code() == Some("40001".into())
                    )
                };

            assert!(
                is_serialization_error(&result_a) || is_serialization_error(&result_b),
                "expected at least one serialization failure, got result_a={:?}, result_b={:?}",
                result_a,
                result_b
            );

            // Cleanup
            sqlx::query("DROP TABLE IF EXISTS ts15_test")
                .execute(&pool)
                .await
                .unwrap();
        }

        /// TS-16-PGSQL: `ReadUncommitted` maps to `read committed`.
        ///
        /// ## What is being tested
        /// PostgreSQL accepts `READ UNCOMMITTED` but internally treats it as
        /// `READ COMMITTED`. The test verifies this mapping by demonstrating that
        /// dirty reads are not possible — the hallmark of true `READ UNCOMMITTED`.
        ///
        /// ## Testing strategy
        /// 1. Create a table `ts16_test` with one row.
        /// 2. Start a transaction with `ReadUncommitted`.
        /// 3. From another connection, start a transaction, update the row, but do **not** commit.
        /// 4. From the first transaction, read the row.
        /// 5. Assert the value is still the original (not the uncommitted update) — proving
        ///    PostgreSQL treated the level as `READ COMMITTED`.
        /// 6. Roll back the second transaction and drop the table.
        #[tokio::test]
        async fn ts_16_read_uncommitted_maps_to_read_committed() {
            let pool = fresh_pool().await;

            // Setup
            sqlx::query("CREATE TABLE ts16_test (id INT PRIMARY KEY, value TEXT)")
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query("INSERT INTO ts16_test (id, value) VALUES (1, 'original')")
                .execute(&pool)
                .await
                .unwrap();

            let man = PostgresTxManager::new(pool.clone())
                .isolated_at(IsolationLevel::ReadUncommitted)
                .if_platform_supports()
                .unwrap();

            // Start transaction A with ReadUncommitted
            let uow_a = man.unit_of_work().await.unwrap();
            let mut uow_a = uow_a;

            // Start transaction B (plain sqlx transaction) and update without committing
            let mut tx_b = pool.begin().await.unwrap();
            sqlx::query("UPDATE ts16_test SET value = 'dirty' WHERE id = 1")
                .execute(tx_b.as_mut())
                .await
                .unwrap();

            // Transaction A reads — should NOT see the uncommitted change
            let row: (String,) = sqlx::query_as("SELECT value FROM ts16_test WHERE id = 1")
                .fetch_one(uow_a.tx_mut().as_mut())
                .await
                .unwrap();
            assert_eq!(
                row.0, "original",
                "ReadUncommitted allowed a dirty read — PostgreSQL did not map it to READ COMMITTED"
            );

            // Roll back transaction B
            tx_b.rollback().await.unwrap();

            // Commit transaction A
            uow_a.commit().await.unwrap();

            // Cleanup
            sqlx::query("DROP TABLE IF EXISTS ts16_test")
                .execute(&pool)
                .await
                .unwrap();
        }
    }

    // ------------------------------------------------------------------
    // Test doubles and helpers (encountered after all scenarios)
    // ------------------------------------------------------------------

    /// Creates a dummy `PostgresTxManager` for unit tests that never touch the database.
    /// Uses `connect_lazy` so no actual connection is attempted.
    fn dummy_manager() -> PostgresTxManager {
        PostgresTxManager {
            pool: PgPool::connect_lazy("postgres://localhost/dummy").expect("dummy pool"),
            isolation_level: None,
        }
    }
}
