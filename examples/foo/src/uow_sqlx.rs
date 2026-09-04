use async_trait::async_trait;
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::uow::{UoW, UoWFac};

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

    async fn with_uow<F, Fut, R>(&self, f: F) -> Result<R, <Self::UoW as UoW>::Error>
    where
        F: FnOnce(Self::UoW) -> Fut + Send,
        Fut: std::future::Future<Output = Result<(Self::UoW, R), <Self::UoW as UoW>::Error>> + Send,
        R: Send,
    {
        let uow = self.create().await?;
        match f(uow).await {
            Ok((uow, value)) => {
                uow.commit().await?;
                Ok(value)
            }
            Err(e) => {
                // Transaction is automatically rolled back when dropped
                Err(e)
            }
        }
    }
}
