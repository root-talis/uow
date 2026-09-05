use thiserror::Error;
use tracing::error;

use crate::{application::port::{BalanceReadError, BalanceRepository}, uow::{TxResult, TxManager}};

pub struct GetBalanceRequest {
    pub user_id: i64,
}

pub type GetBalanceResult = u32;

#[derive(Error, Debug)]
pub enum GetBalanceError<U: crate::uow::UoW> {
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
    tr: M,
    balance_repo: BaR,
}

impl <M, BaR> GetBalanceUsecase<M, BaR>
where
    M: TxManager,
    BaR: BalanceRepository<M::UoW>
{
    pub fn new(tr: M, balance_repo: BaR) -> Self {
        Self { tr, balance_repo }
    }

    pub async fn execute(&self, request: GetBalanceRequest) -> Result<GetBalanceResult, GetBalanceError<M::UoW>> {
        self.tr.transaction(|mut uow| async {
            let r = async {
                let balance = self.balance_repo
                    .read_balance_and_lock(&mut uow, request.user_id).await
                    .map_err(|e| TxResult::Rollback(GetBalanceError::BalanceReadError(e)))?
                    .ok_or(TxResult::Rollback(GetBalanceError::AccountNotAvailable { user_id: request.user_id }))?
                ;

                Ok(TxResult::Commit(balance))
            }.await;

            (uow, r)
        })
        .await
        .map_err(|e| {
            error!(e = e.to_string(), "failed to commit transaction");
            GetBalanceError::PersistenceError(e)
        })?
    }
}

