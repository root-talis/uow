use async_trait::async_trait;
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::{IsloationLevelNotSupportedError, IsolationLevel, IsolationLevelSelector, TransactionError, TxManager, UoW};

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
}

#[async_trait]
impl UoW for SqliteUoW {
    type Error = sqlx::Error;

    async fn commit(self) -> Result<(), Self::Error> {
        self.tx.commit().await
    }
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

#[async_trait]
impl TxManager for SqliteTxManager {
    type UoW = SqliteUoW;

    async fn unit_of_work(&self) -> Result<Self::UoW, <Self::UoW as UoW>::Error> {
        let tx = self.pool.begin().await?;
        Ok(SqliteUoW::new(tx))
    }

    async fn transaction<F, Fut, R, E>(&self, f: F) -> Result<Result<R, E>, TransactionError<Self::UoW>>
    where
        F: FnOnce(Self::UoW) -> Fut + Send,
        Fut: std::future::Future<Output = (Self::UoW, Result<R, E>)> + Send,
        R: Send,
        E: Send,
    {
        let uow = self.unit_of_work().await.map_err(TransactionError::BeginFailed)?;
        let (uow, r) = f(uow).await;
        match r {
            Ok(ok) => {
                // debug!("ok - commit");
                uow.commit().await.map_err(TransactionError::CommitFailed)?;
                Ok(Ok(ok))
            }
            Err(err) => {
                // Transaction is automatically rolled back when dropped
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
            return Err(IsloationLevelNotSupportedError{});
        }

        self.man.isolation_level = self.level;
        Ok(self.man)
    }
}
