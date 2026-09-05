use async_trait::async_trait;
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::uow::{TxResult, UoW, UoWFac};

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
        Fut: std::future::Future<Output = (Self::UoW, Result<TxResult<R>, TxResult<E>>)> + Send,
        R: Send,
        E: Send,
    {
        let uow = self.create().await?;
        let (uow, r) = f(uow).await;
        match r {
            Ok(TxResult::Commit(ok)) => {
                // debug!("ok - commit");
                uow.commit().await?;
                Ok(Ok(ok))
            }
            Err(TxResult::Commit(err)) => {
                // debug!("err - commit");
                uow.commit().await?;
                Ok(Err(err))
            }
            Ok(TxResult::Rollback(ok)) => {
                // debug!("ok - rollback");
                // Transaction is automatically rolled back when dropped
                Ok(Ok(ok))
            }
            Err(TxResult::Rollback(err)) => {
                // debug!("err - rollback");
                // Transaction is automatically rolled back when dropped
                Ok(Err(err))
            }
        }
    }
}
