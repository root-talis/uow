use async_trait::async_trait;

use crate::application::ports::{BalanceReadError, BalanceRepository, TransferRepository};
use uow::sqlx::sqlite::SqliteUoW;


#[derive(Clone)]
pub struct SqliteRepository;


#[async_trait]
impl BalanceRepository<SqliteUoW> for SqliteRepository {
    async fn read_balance(&self, uow: &mut SqliteUoW, user_id: i64) -> Result<Option<u32>, BalanceReadError<SqliteUoW>> {
        struct Balance {
            balance: i64
        }

        let row = sqlx::query_as!(Balance, "SELECT balance FROM accounts WHERE id = $1", user_id)
            .fetch_optional(&mut **uow.tx_mut())
            .await
            .map_err(BalanceReadError::PlatformError)?
        ;

        let balance = row
            .map(|v| v.balance.try_into())
            .transpose()?
        ;

        Ok(balance)
    }
}


#[async_trait]
impl TransferRepository<SqliteUoW> for SqliteRepository {
    async fn withdraw(&self, uow: &mut SqliteUoW, user_id: i64, amount: u32) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE accounts SET balance = balance - $1 WHERE id = $2")
            .bind(amount)
            .bind(user_id)
            .execute(&mut **uow.tx_mut())
            .await?
        ;

        Ok(())
    }

    async fn deposit(&self, uow: &mut SqliteUoW, user_id: i64, amount: u32) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE accounts SET balance = balance + $1 WHERE id = $2")
            .bind(amount)
            .bind(user_id)
            .execute(&mut **uow.tx_mut())
            .await?
        ;

        Ok(())
    }
}
