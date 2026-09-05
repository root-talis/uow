use thiserror::Error;

use crate::{application::{errors::StorageError, ports::AccountsRepository}, domain::value_objects::{AccountNumber, CreditsAmount}};

use uow::{TxManager, UoW};

#[derive(Clone)]
pub struct GetBalanceUsecase<M, A> 
where
    M: TxManager,
    A: AccountsRepository<M::UoW>
{
    tx_manager: M,
    accounts_repo: A,
}

impl <M, BaR> GetBalanceUsecase<M, BaR>
where
    M: TxManager,
    BaR: AccountsRepository<M::UoW>
{
    pub fn new(tx_manager: M, accounts_repo: BaR) -> Self {
        Self { tx_manager, accounts_repo }
    }

    pub async fn execute(&self, account_number: AccountNumber) -> Result<CreditsAmount, GetBalanceError<M::UoW>> {
        let mut uow = self.tx_manager.unit_of_work()
            .await
            .map_err(|e| GetBalanceError::StorageError(StorageError::PlatformError(e)))?;

        let account = self.accounts_repo
            .get_account(&mut uow, account_number)
            .await?
            .ok_or(GetBalanceError::AccountNotAvailable(account_number))?;

        Ok(*account.balance())
    }
}

#[derive(Error, Debug)]
pub enum GetBalanceError<U: UoW> {
    #[error("account not available: {_0}")]
    AccountNotAvailable(AccountNumber),

    #[error("internal error")]
    StorageError(#[from] #[source] StorageError<U>),
}
