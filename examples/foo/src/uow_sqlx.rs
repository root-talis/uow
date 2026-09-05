use async_trait::async_trait;
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::uow::{IsloationLevelNotSupportedError, IsolationLevel, IsolationLevelSelector, TransactionError, TxManager, UoW};

/// SQLx-specific UoW implementation wrapping a SQLite transaction.
#[derive(Debug)]
pub struct SqlxUoW {
    tx: Transaction<'static, Sqlite>,
}

impl SqlxUoW {
    pub fn new(tx: Transaction<'static, Sqlite>) -> Self {
        Self { tx }
    }

    pub fn tx_mut(&mut self) -> &mut Transaction<'static, Sqlite> {
        &mut self.tx
    }
}

#[async_trait]
impl UoW for SqlxUoW {
    type Error = sqlx::Error;

    async fn commit(self) -> Result<(), Self::Error> {
        self.tx.commit().await
    }
}

/// SQLx-specific UoW factory.
#[derive(Clone)]
pub struct SqlxTxManager {
    pool: SqlitePool,
    isolation_level: Option<IsolationLevel>,
}

impl SqlxTxManager {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            isolation_level: None,
        }
    }
}

#[async_trait]
impl TxManager for SqlxTxManager {
    type UoW = SqlxUoW;

    async fn unit_of_work(&self) -> Result<Self::UoW, <Self::UoW as UoW>::Error> {
        let tx = self.pool.begin().await?;
        Ok(SqlxUoW::new(tx))
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
        SqlxIsolationLevelSelector::new(self.clone(), level)
    }
}

pub struct SqlxIsolationLevelSelector {
    man: SqlxTxManager,
    level: Option<IsolationLevel>,
}

impl SqlxIsolationLevelSelector {
    fn new(man: SqlxTxManager, level: IsolationLevel) -> Self {
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

impl IsolationLevelSelector<SqlxTxManager> for SqlxIsolationLevelSelector {

    fn or(mut self, level: IsolationLevel) -> Self {
        self.level = self.level.or_else(|| Self::if_supported(level));
        self
    }

    fn if_platform_supports(mut self) -> Result<SqlxTxManager, IsloationLevelNotSupportedError> {
        if self.level.is_none() {
            return Err(IsloationLevelNotSupportedError{});
        }

        self.man.isolation_level = self.level;
        Ok(self.man)
    }
}
