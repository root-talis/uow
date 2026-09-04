use async_trait::async_trait;

use crate::application::TransferRepository;
use crate::uow_sqlx::SqlxUoW;


#[derive(Clone)]
pub struct SqlxTransferRepository;

#[async_trait]
impl TransferRepository<SqlxUoW> for SqlxTransferRepository {
    async fn withdraw(
        &self,
        uow: &mut SqlxUoW,
        user_id: i64,
        amount: u32,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE accounts SET balance = balance - $1 WHERE id = $2")
            .bind(amount)
            .bind(user_id)
            .execute(&mut **uow.tx_mut())
            .await?
        ;

        Ok(())
    }

    async fn deposit(
        &self,
        uow: &mut SqlxUoW,
        user_id: i64,
        amount: u32,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE accounts SET balance = balance + $1 WHERE id = $2")
            .bind(amount)
            .bind(user_id)
            .execute(&mut **uow.tx_mut())
            .await?
        ;

        Ok(())
    }
}
