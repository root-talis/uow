use async_trait::async_trait;
use thiserror::Error;
use tracing::error;

use crate::uow::{TxResult, UoWFac};

pub struct TransferRequest {
    pub from_user_id: i64,
    pub to_user_id: i64,
    pub amount: u32,
}

#[derive(Clone)]
pub struct TransferUsecase<F: UoWFac, R: TransferRepository<F::UoW> + Clone> {
    uow_fac: F,
    repository: R,
}

impl<F: UoWFac, R: TransferRepository<F::UoW> + Clone> TransferUsecase<F, R> {
    pub fn new(uow_fac: F, repository: R) -> Self {
        Self { uow_fac, repository }
    }

    pub async fn execute(
        &self,
        payload: TransferRequest,
    ) -> Result<(), TransferError<F::UoW>> {
        self.uow_fac.with_uow(|mut uow| async {
            self.repository
                .withdraw(&mut uow, payload.from_user_id, payload.amount)
                .await
                .map_err(|e| TxResult::Rollback(uow, TransferError::PersistenceError(e)))?;

            self.repository
                .deposit(&mut uow, payload.to_user_id, payload.amount)
                .await
                .map_err(|e| TxResult::Rollback(uow, TransferError::PersistenceError(e)))?;

            Ok(TxResult::Commit(uow, ()))
        })
        .await
        .map_err(|e| {
            error!(e = e.to_string(), "failed to commit transaction");
            TransferError::PersistenceError(e)
        })?
    }
}

#[derive(Error)]
enum TransferError<U: crate::uow::UoW> {
    #[error("source account has insufficient funds")]
    InsufficientFunds {src_user_id: i64},

    #[error("persistence error")]
    PersistenceError(U::Error),
}


#[async_trait]
pub trait TransferRepository<U: crate::uow::UoW>: Send + Sync {
    async fn withdraw(
        &self,
        uow: &mut U,
        user_id: i64,
        amount: u32,
    ) -> Result<(), U::Error>;

    async fn deposit(
        &self,
        uow: &mut U,
        user_id: i64,
        amount: u32,
    ) -> Result<(), U::Error>;
}
