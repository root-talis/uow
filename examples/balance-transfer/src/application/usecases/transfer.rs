use thiserror::Error;
use tracing::error;
use uow::{transaction, IsloationLevelNotSupportedError, IsolationLevel, IsolationLevelSelector, TxManager, UoW};

use crate::{application::{errors::StorageError, ports::AccountsRepository}, domain::{self, services::transfer_credits, value_objects::{AccountNumber, CreditsAmount}}};

pub struct TransferRequest {
    pub from:   AccountNumber,
    pub to:     AccountNumber,
    pub amount: CreditsAmount,
}

pub type TransferResult = ();

#[derive(Error, Debug)]
pub enum TransferError<U: UoW> {
    #[error("account not available: {_0}")]
    AccountNotAvailable(AccountNumber),

    #[error("failed to transfer credits: {_0}")]
    TransferFailed(#[from] #[source] domain::errors::TransferFailure),

    #[error("internal error")]
    IsolationLevelNotSupported(#[from] IsloationLevelNotSupportedError),

    #[error("internal error")]
    StorageError(#[from] #[source] StorageError<U>),
}

#[derive(Clone)]
pub struct TransferUsecase<M, A> 
where
    M: TxManager,
    A: AccountsRepository<M::UoW>
{
    tx_manager: M,
    accounts_repo: A,
}

impl<M, A> TransferUsecase<M, A>
where
    M: TxManager,
    A: AccountsRepository<M::UoW>
{
    pub fn new(tx_manager: M, accounts_repo: A) -> Self {
        Self { tx_manager, accounts_repo }
    }

    pub async fn execute(&self, request: TransferRequest) -> Result<TransferResult, TransferError<M::UoW>> {
        transaction!(
            self.tx_manager
                .isolated_at(IsolationLevel::RepeatableRead)
                    .or(IsolationLevel::Serializable)
                    .if_platform_supports()?,
            |mut uow| async {
                let mut from_account = self.accounts_repo
                    .get_account(&mut uow, request.from)
                    .await?
                    .ok_or(TransferError::AccountNotAvailable(request.from))?
                ;

                let mut to_account = self.accounts_repo
                    .get_account(&mut uow, request.to)
                    .await?
                    .ok_or(TransferError::AccountNotAvailable(request.to))?
                ;

                transfer_credits(&mut from_account, request.amount, &mut to_account)?;

                self.accounts_repo.save_account(&mut uow, &from_account).await?;
                self.accounts_repo.save_account(&mut uow, &to_account).await?;

                Ok(())
            }
        )
        .await
        .map_err(|e| {
            error!(e = e.to_string(), "transaction error");
            TransferError::StorageError(StorageError::PlatformError(e.into_inner()))
        })?
    }
}
