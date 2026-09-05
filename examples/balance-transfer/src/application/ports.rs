use std::num::TryFromIntError;

use async_trait::async_trait;
use thiserror::Error;

use uow::UoW;

#[async_trait]
pub trait TransferRepository<U: UoW>: Clone + Send + Sync {
    async fn withdraw(&self, uow: &mut U, user_id: i64, amount: u32) -> Result<(), U::Error>;

    async fn deposit(&self, uow: &mut U, user_id: i64, amount: u32) -> Result<(), U::Error>;
}

#[async_trait]
pub trait BalanceRepository<U: UoW>: Clone + Send + Sync {
    async fn read_balance(&self, uow: &mut U, user_id: i64) -> Result<Option<u32>, BalanceReadError<U>>;
}

#[derive(Error, Debug)]
pub enum BalanceReadError<U: UoW> {
    #[error("error fetching balance")]
    PlatformError(#[source] U::Error),

    #[error("error parsing balance")]
    OutOfBoundsError(#[from] #[source] TryFromIntError),
}
