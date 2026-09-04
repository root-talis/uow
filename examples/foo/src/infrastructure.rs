use async_trait::async_trait;

use crate::uow::UoW;

#[async_trait]
pub trait TransferRepository: Send + Sync {
    async fn withdraw(
        &self,
        uow: &mut UoW<'_>,
        user_id: i64,
        amount: i64,
    ) -> Result<(), sqlx::Error>;

    async fn deposit(
        &self,
        uow: &mut UoW<'_>,
        user_id: i64,
        amount: i64,
    ) -> Result<(), sqlx::Error>;
}

#[derive(Clone)]
pub struct SqlxTransferRepository;

#[async_trait]
impl TransferRepository for SqlxTransferRepository {
    async fn withdraw(
        &self,
        uow: &mut UoW<'_>,
        user_id: i64,
        amount: i64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE accounts SET balance = balance - $1 WHERE id = $2")
            .bind(amount)
            .bind(user_id)
            .execute(&mut **uow.tx_mut())
            .await?;

        Ok(())
    }

    async fn deposit(
        &self,
        uow: &mut UoW<'_>,
        user_id: i64,
        amount: i64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE accounts SET balance = balance + $1 WHERE id = $2")
            .bind(amount)
            .bind(user_id)
            .execute(&mut **uow.tx_mut())
            .await?;

        Ok(())
    }
}
