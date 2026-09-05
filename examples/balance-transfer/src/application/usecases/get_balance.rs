use thiserror::Error;

use crate::application::ports::{BalanceReadError, BalanceRepository};

use uow::{TxManager, UoW};

pub struct GetBalanceRequest {
    pub user_id: i64,
}

pub type GetBalanceResult = u32;

#[derive(Error, Debug)]
pub enum GetBalanceError<U: UoW> {
    #[error("persistence error")]
    PersistenceError(U::Error),

    #[error(transparent)]
    BalanceReadError(#[from] BalanceReadError<U>),

    #[error("account not available: {user_id}")]
    AccountNotAvailable{ user_id: i64 },
}

#[derive(Clone)]
pub struct GetBalanceUsecase<M, BaR> 
where
    M: TxManager,
    BaR: BalanceRepository<M::UoW>
{
    tx_manager: M,
    balance_repo: BaR,
}

impl <M, BaR> GetBalanceUsecase<M, BaR>
where
    M: TxManager,
    BaR: BalanceRepository<M::UoW>
{
    pub fn new(tx_manager: M, balance_repo: BaR) -> Self {
        Self { tx_manager, balance_repo }
    }

    pub async fn execute(&self, request: GetBalanceRequest) -> Result<GetBalanceResult, GetBalanceError<M::UoW>> {
        let mut uow = self.tx_manager.unit_of_work()
            .await
            .map_err(GetBalanceError::PersistenceError)?;

        let balance = self.balance_repo
            .read_balance(&mut uow, request.user_id).await
            .map_err(|e| GetBalanceError::BalanceReadError(e))?
            .ok_or(GetBalanceError::AccountNotAvailable { user_id: request.user_id })?
        ;

        Ok(balance)
    }
}
