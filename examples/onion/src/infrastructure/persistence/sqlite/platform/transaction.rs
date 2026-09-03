use sqlx::{Sqlite, Transaction};

pub struct SqliteTransaction<'c> {
    pub tx: Transaction<'c, Sqlite>,
}

impl <'c> SqliteTransaction<'c> {
    pub fn new(tx: Transaction<'c, Sqlite>) -> Self {
        Self { tx }
    }
}

/// Trait that allows committing or rolling back a transaction.
/// This trait is private to the transaction module so that only
/// SqliteUnitOfWorkFactory (which lives in this module) can call these methods.
pub(crate) trait Commit {
    async fn commit(self) -> Result<(), sqlx::Error>;
    async fn rollback(self) -> Result<(), sqlx::Error>;
}

impl <'c> Commit for SqliteTransaction<'c> {
    async fn commit(self) -> Result<(), sqlx::Error> {
        self.tx.commit().await
    }

    async fn rollback(self) -> Result<(), sqlx::Error> {
        self.tx.rollback().await
    }
}
