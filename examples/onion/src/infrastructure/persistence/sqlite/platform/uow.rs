use std::sync::Arc;

use sqlx::{Connection, Pool, Sqlite};
use ::uow::UnitOfWork;
use uow::{TxResult, UnitOfWorkFactory};
use super::SqliteTransaction;

use crate::infrastructure::persistence::sqlite::platform::{SqlitePlatform};

pub struct SqliteUnitOfWork<'c> {
    tx: SqliteTransaction<'c>,
}

impl <'c> SqliteUnitOfWork<'c>  {
    /// Creates a new sqlite unit of work with the given transaction.
    pub fn new(tx: SqliteTransaction<'c>) -> Self {
        Self { tx: tx }
    }

    /// Returns a mutable reference to the transaction.
    pub fn tx_mut(&'c mut self) -> &mut SqliteTransaction {
        &mut self.tx
    }

    /// Returns an immutable reference to the transaction.
    pub fn tx(&self) -> &SqliteTransaction {
        &self.tx
    }
}

impl <'c> UnitOfWork<SqlitePlatform> for SqliteUnitOfWork<'c> {}


/// Sqlite unit of work factory.
/// Owns a connection pool.
pub struct SqliteUnitOfWorkFactory {
    pool: Arc<Pool<Sqlite>>,
}

impl SqliteUnitOfWorkFactory {
    /// Creates a new factory with the given pool.
    pub fn new(pool: Arc<Pool<Sqlite>>) -> Self {
        Self { pool }
    }
}

use super::transaction::Commit;

impl UnitOfWorkFactory<SqlitePlatform> for SqliteUnitOfWorkFactory {
    type UoW<'c> = SqliteUnitOfWork<'c>;

    async fn with_unitofwork<T, E, F>(&self, f: F) -> Result<T, E>
    where
        T: Send,
        E: Send,
        F: for <'c> AsyncFnOnce(&mut Self::UoW<'c>) -> Result<TxResult<T>, TxResult<E>> + Send,
    {
        let mut conn = self.pool.acquire().await.expect("fix error handling here");

        let sqlx_tx = conn.begin().await.expect("fix error handling here");
        let uow_tx = SqliteTransaction::new(sqlx_tx);

        let mut uow = SqliteUnitOfWork::new(uow_tx);
        let result = f(&mut uow).await;
        let uow_tx = uow.tx;

        match result {
            Ok(TxResult::Commit(t)) => {
                uow_tx.commit().await.expect("fix error handling here");
                Ok(t)
            }
            Ok(TxResult::Rollback(t)) => {
                uow_tx.rollback().await.expect("fix error handling here");
                Ok(t)
            }
            Err(TxResult::Commit(e)) => {
                uow_tx.commit().await.expect("fix error handling here");
                Err(e)
            }
            Err(TxResult::Rollback(e)) => {
                uow_tx.rollback().await.expect("fix error handling here");
                Err(e)
            }
        }
    }
}
