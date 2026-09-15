use thiserror::Error;
use tracing::error;
use uow::{TxManager, UoW, transaction};

use crate::{
    application::{errors::StorageError, ports::AccountsRepository},
    domain::value_objects::{AccountNumber, CreditsAmount},
};

#[derive(Clone)]
pub struct GetBalanceUsecase<M, A>
where
    M: TxManager,
    A: AccountsRepository<M::UoW>,
{
    tx_manager: M,
    accounts_repo: A,
}

impl<M, A> GetBalanceUsecase<M, A>
where
    M: TxManager,
    A: AccountsRepository<M::UoW>,
{
    pub fn new(tx_manager: M, accounts_repo: A) -> Self {
        Self {
            tx_manager,
            accounts_repo,
        }
    }

    pub async fn execute(
        &self,
        account_number: AccountNumber,
    ) -> Result<CreditsAmount, GetBalanceError<M::UoW>> {
        transaction!(self.tx_manager, |mut uow| async {
            let account = self
                .accounts_repo
                .get_account(&mut uow, account_number)
                .await?
                .ok_or(GetBalanceError::AccountNotAvailable(account_number))?;

            Ok(*account.balance())
        })
        .await
        .map_err(|e| {
            error!(e = e.to_string(), "transaction error");
            GetBalanceError::StorageError(StorageError::PlatformError(e.into_inner()))
        })?
    }
}

#[derive(Error, Debug)]
pub enum GetBalanceError<U: UoW> {
    #[error("account not available: {_0}")]
    AccountNotAvailable(AccountNumber),

    #[error("internal error")]
    StorageError(
        #[from]
        #[source]
        StorageError<U>,
    ),
}
