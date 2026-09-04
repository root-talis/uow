use async_trait::async_trait;
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::uow::{TxResult, UoW, UoWFac};

/// SQLx-specific UoW implementation wrapping a SQLite transaction.
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
pub struct SqlxUoWFac {
    pool: SqlitePool,
}

impl SqlxUoWFac {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UoWFac for SqlxUoWFac {
    type UoW = SqlxUoW;

    async fn create(&self) -> Result<Self::UoW, <Self::UoW as UoW>::Error> {
        let tx = self.pool.begin().await?;
        Ok(SqlxUoW::new(tx))
    }

    async fn with_uow<F, Fut, R, E>(&self, f: F) -> Result<Result<R, E>, <Self::UoW as UoW>::Error>
    where
        F: FnOnce(Self::UoW) -> Fut + Send,
        Fut: std::future::Future<Output = Result<TxResult<Self::UoW, R>, TxResult<Self::UoW, E>>> + Send,
        R: Send,
        E: Send,
    {
        let uow = self.create().await?;
        match f(uow).await {
            Ok(TxResult::Commit(uow, value)) => {
                uow.commit().await?;
                Ok(Ok(value))
            }
            Err(TxResult::Commit(uow, err)) => {
                uow.commit().await?;
                Ok(Err(err))
            }
            Ok(TxResult::Rollback(_uow, value)) => {
                // Transaction is automatically rolled back when dropped
                Ok(Ok(value))
            }
            Err(TxResult::Rollback(_uow, err)) => {
                // Transaction is automatically rolled back when dropped
                Ok(Err(err))
            }
        }
    }
}
