use async_trait::async_trait;

use crate::application::ports::{BalanceReadError, BalanceRepository, TransferRepository};
use crate::uow_sqlx::SqlxUoW;


#[derive(Clone)]
pub struct SqlxRepository;


#[async_trait]
impl BalanceRepository<SqlxUoW> for SqlxRepository {
    async fn read_balance_and_lock(&self, uow: &mut SqlxUoW, user_id: i64) -> Result<Option<u32>, BalanceReadError<SqlxUoW>> {
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
impl TransferRepository<SqlxUoW> for SqlxRepository {
    async fn withdraw(&self, uow: &mut SqlxUoW, user_id: i64, amount: u32) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE accounts SET balance = balance - $1 WHERE id = $2")
            .bind(amount)
            .bind(user_id)
            .execute(&mut **uow.tx_mut())
            .await?
        ;

        Ok(())
    }

    async fn deposit(&self, uow: &mut SqlxUoW, user_id: i64, amount: u32) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE accounts SET balance = balance + $1 WHERE id = $2")
            .bind(amount)
            .bind(user_id)
            .execute(&mut **uow.tx_mut())
            .await?
        ;

        Ok(())
    }
}
