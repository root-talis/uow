use sqlx::{Sqlite, SqlitePool, Transaction};

#[derive(Clone)]
pub struct UoWFac {
    pool: SqlitePool,
}

impl UoWFac {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create(&self) -> Result<UoW<'_>, sqlx::Error> {
        let tx = self.pool.begin().await?;
        Ok(UoW { tx })
    }
}

pub struct UoW<'a> {
    tx: Transaction<'a, Sqlite>,
}

impl<'a> UoW<'a> {
    pub async fn commit(self) -> Result<(), sqlx::Error> {
        self.tx.commit().await
    }

    pub fn tx_mut(&mut self) -> &mut Transaction<'a, Sqlite> {
        &mut self.tx
    }
}
