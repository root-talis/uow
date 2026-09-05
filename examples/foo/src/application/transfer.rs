use thiserror::Error;
use tracing::error;

use crate::{application::port::{BalanceReadError, BalanceRepository, TransferRepository}, uow::{TxManager, TxResult, UoW}};

pub struct TransferRequest {
    pub from_user_id: i64,
    pub to_user_id: i64,
    pub amount: u32,
}

pub type TransferResult = ();

#[derive(Error, Debug)]
pub enum TransferError<U: crate::uow::UoW> {
    #[error("source account has insufficient funds")]
    InsufficientFunds {src_user_id: i64},

    #[error("persistence error")]
    PersistenceError(U::Error),

    #[error(transparent)]
    BalanceReadError(#[from] BalanceReadError<U>),

    #[error("account not available: {user_id}")]
    AccountNotAvailable{ user_id: i64 },
}

#[derive(Clone)]
pub struct TransferUsecase<M, TrR, BaR> 
where
    M: TxManager,
    TrR: TransferRepository<M::UoW>,
    BaR: BalanceRepository<M::UoW>
{
    tr: M,
    transfer_repo: TrR,
    balance_repo: BaR,
}

impl<M, TrR, BaR> TransferUsecase<M, TrR, BaR>
where
    M: TxManager,
    TrR: TransferRepository<M::UoW>,
    BaR: BalanceRepository<M::UoW>
{
    pub fn new(tr: M, transfer_repo: TrR, balance_repo: BaR) -> Self {
        Self { tr, transfer_repo, balance_repo }
    }

    pub async fn execute(
        &self,
        request: TransferRequest,
    ) -> Result<TransferResult, TransferError<M::UoW>> {
        self.tr.transaction(|mut uow| async {
            let r = async {
                let src_balance = self.balance_repo
                    .read_balance_and_lock(&mut uow, request.from_user_id).await
                    .map_err(|e| TxResult::Rollback(TransferError::BalanceReadError(e)))?
                    .ok_or(TxResult::Rollback(TransferError::AccountNotAvailable { user_id: request.from_user_id }))?
                ;

                if src_balance < request.amount {
                    return Err(TxResult::Rollback(TransferError::InsufficientFunds { src_user_id: request.from_user_id }))
                }

                self.balance_repo
                    .read_balance_and_lock(&mut uow, request.to_user_id).await
                    .map_err(|e| TxResult::Rollback(TransferError::BalanceReadError(e)))?
                    .ok_or(TxResult::Rollback(TransferError::AccountNotAvailable { user_id: request.to_user_id }))?
                ;

                self.transfer_repo
                    .withdraw(&mut uow, request.from_user_id, request.amount)
                    .await
                    .map_err(|e| TxResult::Rollback(TransferError::PersistenceError(e)))?;

                self.transfer_repo
                    .deposit(&mut uow, request.to_user_id, request.amount)
                    .await
                    .map_err(|e| TxResult::Rollback(TransferError::PersistenceError(e)))?;

                Ok(TxResult::Commit(()))
            }.await;

            (uow, r)
        })
        .await
        .map_err(|e| {
            error!(e = e.to_string(), "failed to commit transaction");
            TransferError::PersistenceError(e)
        })?
    }
}
